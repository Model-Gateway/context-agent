// Port of apps/macos/ContextAgent/App/ProjectState.swift (the `ProjectState`
// class; its `AppState` extension of project-level intents lives in
// state/app.ts) and of `FileTreeModel` (Views/Inspector/FilesTab.swift), the
// Inspector Files tab's lazily loaded tree, which the Swift app keeps on the
// project so expanded folders survive tab switches.
//
// Like `ChatState`, `ProjectState` is a plain class mutated in place; every
// change ends with `touch()`, which bumps `version` and tells the app store
// (state/app.ts wires `ProjectState.changeSink`). The backend loads
// (`loadSessions`, `refreshGit`, …) talk to the app's one `BackendClient`.

import { invoke } from '@tauri-apps/api/core'
import { backend } from '../backend/client'
import { errorText } from '../backend/errors'
import {
  runningSessionIds as infoRunningSessionIds,
  sessionUpdatedDate,
  type DoctorReport,
  type GitBranches,
  type GitDiff,
  type GitLogEntry,
  type GitStatus,
  type IndexProgress,
  type IndexReady,
  type MemoryEntry,
  type ProjectInfo,
  type SessionSummary,
  type StarterSuggestion,
  type TaskSummary,
  type WorktreeEntry,
} from '../backend/types'
import { invalidateFileReferences } from '../markdown/fileReferences'
import type { ModelCatalog } from '../models/modelCatalog'
import { chatStatusIsRunning } from '../models/reducer'
import { ChatState } from './chat'
import type { SidebarItem } from './app'

// MARK: - ProjectState

/// `ProjectState` (App/ProjectState.swift): per-project state: sessions and
/// their chats, index / git status, the model catalog, doctor, memories and
/// tasks. Chats are created once and never replaced (`chat(sessionId)`
/// always returns the same instance).
export class ProjectState {
  /// Called after every `touch()` of any project; state/app.ts sets it to
  /// bump the store. Null outside the app (tests).
  static changeSink: ((project: ProjectState) => void) | null = null

  readonly id: string
  info: ProjectInfo

  sessions: SessionSummary[] = []
  sessionsLoading = false
  /** Inline error of the last `sessions/list`. */
  sessionsError: string | null = null

  private chats = new Map<string, ChatState>()

  indexProgress: IndexProgress | null = null
  lastIndexReady: IndexReady | null = null

  gitStatus: GitStatus | null = null
  gitError: string | null = null
  selectedChangePath: string | null = null
  diff: GitDiff | null = null
  diffLoading = false
  diffError: string | null = null
  /** Changes tab "staged" toggle. */
  showStaged = false

  // Git actions (Changes tab; see state/git.ts)
  /** The commit box's draft. */
  commitMessage = ''
  /** Label of the git action in flight ("Pushing…"), null when idle. */
  gitBusy: string | null = null
  generatingMessage = false
  branches: GitBranches | null = null
  commitLog: GitLogEntry[] = []
  /** Commit was asked with nothing staged: the view shows "stage all and commit?". */
  askCommitAll = false

  /// Sessions with a task running: every session of a project can run at
  /// the same time, in the same checkout. Replaced, never mutated in place.
  runningSessionIds: Set<string>

  models: ModelCatalog
  /** The model name a `models/switch` is probing, while it runs. */
  switchingModel: string | null = null

  doctor: DoctorReport | null = null
  doctorRunning = false
  doctorError: string | null = null

  memories: MemoryEntry[] = []
  tasks: TaskSummary[] = []
  extrasLoading = false
  extrasError: string | null = null

  lastTab: SidebarItem = 'chat'

  /// `suggest/starters` under the empty chat's composer; null until the
  /// first answer (the list shows its skeleton while `startersLoading`).
  starters: StarterSuggestion[] | null = null
  startersLoading = false
  startersLoadedAt: Date | null = null

  /** Every worktree of the repository (`worktree/list`), refreshed with git status. */
  worktrees: WorktreeEntry[] = []
  /** True while `worktree/create` runs for this project. */
  creatingWorktree = false

  /** Called for every newly created chat (AppState wires visibility). */
  onChatCreated: ((chat: ChatState) => void) | null = null

  /// The `sessions/list` in flight, so a caller that needs the list can
  /// await it instead of skipping the load (session restore at launch).
  sessionsLoad: Promise<void> | null = null

  /** Bumped by every `touch()` (and every touch of one of its chats). */
  version = 0

  private _fileTree: FileTreeModel | null = null

  constructor(info: ProjectInfo, models: ModelCatalog) {
    this.id = info.project_id
    this.info = info
    this.models = models
    this.runningSessionIds = infoRunningSessionIds(info)
  }

  // MARK: - Observation

  /** Marks the project changed: bumps `version` and tells the store. */
  touch(): void {
    this.version += 1
    ProjectState.changeSink?.(this)
  }

  /** Runs `change` on the project, then touches it. */
  update(change: (project: ProjectState) => void): void {
    change(this)
    this.touch()
  }

  /** Stops every chat's pending delta flush (the project closed). */
  dispose(): void {
    for (const chat of this.chats.values()) chat.dispose()
  }

  // MARK: - Derived

  get name(): string {
    return this.info.name
  }

  get root(): string {
    return this.info.root
  }

  get branch(): string | null {
    return this.gitStatus?.branch ?? this.info.git?.branch ?? null
  }

  get isGitRepository(): boolean {
    return this.info.git != null
  }

  /// Whether any session of the project is running a task (model switch,
  /// index rebuild, git rewrites and merges wait for all of them).
  get isRunning(): boolean {
    return this.runningSessionIds.size > 0
  }

  /** `isRunning(session:)`. */
  isRunningSession(sessionId: string): boolean {
    return this.runningSessionIds.has(sessionId)
  }

  /** `runningSessionIds.insert(id)` (a new set, so memoised readers see the change). */
  addRunning(sessionId: string): void {
    if (this.runningSessionIds.has(sessionId)) return
    this.runningSessionIds = new Set([...this.runningSessionIds, sessionId])
  }

  /** `runningSessionIds.remove(id)`. */
  removeRunning(sessionId: string): void {
    if (!this.runningSessionIds.has(sessionId)) return
    const next = new Set(this.runningSessionIds)
    next.delete(sessionId)
    this.runningSessionIds = next
  }

  // MARK: Worktrees

  /** The main checkout's root when this project is a linked worktree. */
  get parentRoot(): string | null {
    return this.info.git?.worktree?.parent_root ?? null
  }

  get isWorktree(): boolean {
    return this.parentRoot !== null
  }

  /** The branch without the `ctx/` prefix `worktree/create` gives it. */
  get worktreeLabel(): string {
    const branch = this.branch
    if (!branch) return this.name
    return branch.startsWith('ctx/') ? branch.slice(4) : branch
  }

  /** Dirty file count for the sidebar chip. */
  get dirtyCount(): number {
    if (this.gitStatus) return this.gitStatus.files.length
    return this.info.git?.dirty_files ?? 0
  }

  /** Lines added / removed across the working tree, for the Inspector Run tab badge. */
  get lineChanges(): { added: number; removed: number } {
    const files = this.gitStatus?.files ?? []
    let added = 0
    let removed = 0
    for (const file of files) {
      added += file.insertions ?? 0
      removed += file.deletions ?? 0
    }
    return { added, removed }
  }

  /** The Inspector Files tab's tree; kept here so expanded folders survive tab switches. */
  get fileTree(): FileTreeModel {
    if (this._fileTree === null || this._fileTree.root !== this.info.root) {
      this._fileTree = new FileTreeModel(this.info.root, () => this.touch())
    }
    return this._fileTree
  }

  get unseenCount(): number {
    let count = 0
    for (const chat of this.chats.values()) if (chat.unseen !== null) count += 1
    return count
  }

  /// Chats that want attention (unseen, or waiting for an approval or an
  /// answer), each counted once however many reasons it has.
  get attentionCount(): number {
    let count = 0
    for (const chat of this.chats.values()) if (chat.unseen !== null || chat.needsHelp) count += 1
    return count
  }

  /** Every chat this project has instantiated (running or not). */
  get allChats(): ChatState[] {
    return [...this.chats.values()]
  }

  /** A session's title: its open chat's, else its summary's. */
  chatTitle(sessionId: string): string | null {
    return this.chats.get(sessionId)?.title ?? this.session(sessionId)?.title ?? null
  }

  /** Chats with queued prompts, earliest queue first. */
  get queuedChats(): ChatState[] {
    const far = Number.POSITIVE_INFINITY
    return [...this.chats.values()]
      .filter((chat) => chat.queued.length > 0)
      .sort((a, b) => (a.queuedSince?.getTime() ?? far) - (b.queuedSince?.getTime() ?? far))
  }

  get pendingApprovalChats(): ChatState[] {
    return [...this.chats.values()].filter((chat) => chat.pendingApproval !== null)
  }

  get hasIndexError(): boolean {
    return this.lastIndexReady?.error != null
  }

  /** "Index ready · 310 files · generation 12" / "Indexing 120/310" / failure. */
  get indexSummary(): string {
    const progress = this.indexProgress
    if (progress && progress.total > 0) return `Indexing ${progress.done}/${progress.total}`
    if (this.info.indexing && this.lastIndexReady === null) return 'Indexing…'
    const error = this.lastIndexReady?.error
    if (error != null) return `Indexing failed: ${error} · retrieval falls back to text tools`
    return `Index ready · ${this.info.index.files} files · generation ${this.info.index.generation}`
  }

  // MARK: - Chats

  existingChat(sessionId: string): ChatState | null {
    return this.chats.get(sessionId) ?? null
  }

  /// The one `ChatState` per session; created on first use from the session
  /// summary (or a placeholder for runs started elsewhere).
  chat(sessionId: string): ChatState {
    const existing = this.chats.get(sessionId)
    if (existing) return existing
    const now = ProjectState.now()
    const summary: SessionSummary = this.session(sessionId) ?? {
      id: sessionId,
      model: this.info.model,
      mode: 'desktop',
      title: 'New chat',
      created_at: now,
      updated_at: now,
      active_task_id: null,
      task_count: 0,
      last_status: null,
    }
    const chat = new ChatState(summary, this.id, sessionId)
    this.chats.set(sessionId, chat)
    if (!this.sessions.some((session) => session.id === sessionId)) {
      this.sessions = [summary, ...this.sessions]
    }
    this.onChatCreated?.(chat)
    this.touch()
    return chat
  }

  session(sessionId: string): SessionSummary | null {
    return this.sessions.find((session) => session.id === sessionId) ?? null
  }

  /** `upsert(session:)`: inserts or replaces a summary (newest first) and syncs the chat's copy. */
  upsertSession(summary: SessionSummary): void {
    const list = [summary, ...this.sessions.filter((session) => session.id !== summary.id)]
    this.sessions = stableSortByUpdated(list)
    const chat = this.chats.get(summary.id)
    if (chat) {
      chat.summary = summary
      chat.touch()
    }
    this.touch()
  }

  replaceSessions(list: SessionSummary[]): void {
    const sessions = [...list]
    for (const summary of list) {
      const chat = this.chats.get(summary.id)
      if (chat) {
        chat.summary = summary
        chat.touch()
      }
    }
    // Keep placeholders for chats that have content but are missing from the list (very new sessions).
    const listed = new Set(list.map((summary) => summary.id))
    for (const [sessionId, chat] of this.chats) {
      if (!listed.has(sessionId) && chat.hasContent) sessions.push(chat.summary)
    }
    this.sessions = sessions
    this.touch()
  }

  removeSession(sessionId: string): void {
    this.sessions = this.sessions.filter((session) => session.id !== sessionId)
    this.chats.get(sessionId)?.dispose()
    this.chats.delete(sessionId)
    this.touch()
  }

  /** Bumps the local summary after a send so the sidebar reorders immediately. */
  touchSession(sessionId: string, firstPrompt: string | null): void {
    const index = this.sessions.findIndex((session) => session.id === sessionId)
    const current = index >= 0 ? this.sessions[index] : undefined
    if (!current) return
    const summary: SessionSummary = { ...current, updated_at: ProjectState.now() }
    if (firstPrompt !== null && summary.task_count === 0 && summary.title === 'New chat') {
      summary.title = ChatState.clip(firstPrompt, 80)
    }
    this.sessions = [summary, ...this.sessions.filter((_, i) => i !== index)]
    const chat = this.chats.get(sessionId)
    if (chat) {
      chat.summary = summary
      chat.touch()
    }
    this.touch()
  }

  // MARK: - Backend loads (called by AppState)

  /** `sessions/list`. A caller that needs the list awaits `sessionsLoad` when one is in flight. */
  async loadSessions(): Promise<void> {
    const load = (async () => {
      this.sessionsLoading = true
      this.touch()
      try {
        const list = await backend.sessionsList(this.id, 100)
        this.replaceSessions(list)
        this.sessionsError = null
      } catch (error) {
        this.sessionsError = errorText(error)
      } finally {
        this.sessionsLoading = false
        this.touch()
      }
    })()
    this.sessionsLoad = load
    this.touch()
    await load
    if (this.sessionsLoad === load) {
      this.sessionsLoad = null
      this.touch()
    }
  }

  /** `git/status` (+ `git/diff` for the selected path). Errors stay inline in the Changes tab. */
  async refreshGit(): Promise<void> {
    if (!this.isGitRepository) {
      this.gitStatus = null
      this.gitError = null
      this.diff = null
      this.touch()
      return
    }
    try {
      const status = await backend.gitStatus(this.id)
      if (!sameJSON(status, this.gitStatus)) invalidateFileReferences()
      this.gitStatus = status
      this.gitError = null
      try {
        this.worktrees = await backend.worktreeList(this.id)
      } catch {
        // Older backends have no worktree/list; keep what we had.
      }
      const path = this.selectedChangePath
      if (path !== null && !status.files.some((file) => file.path === path)) {
        this.selectedChangePath = null
        this.diff = null
      }
      this.touch()
      if (this.selectedChangePath !== null) await this.loadDiff()
    } catch (error) {
      this.gitError = errorText(error)
      this.touch()
    }
  }

  async loadDiff(): Promise<void> {
    const path = this.selectedChangePath
    if (path === null) {
      this.diff = null
      this.touch()
      return
    }
    this.diffLoading = true
    this.touch()
    try {
      const result = await backend.gitDiff(this.id, path, this.showStaged)
      // The selection may have moved while the diff was loading.
      if (this.selectedChangePath === path) {
        this.diff = result
        this.diffError = null
      }
    } catch (error) {
      if (this.selectedChangePath === path) {
        this.diff = null
        this.diffError = errorText(error)
      }
    } finally {
      this.diffLoading = false
      this.touch()
    }
  }

  /** Fresh `project/status` (index stats, git, model). */
  async refreshStatus(): Promise<void> {
    let fresh: ProjectInfo
    try {
      fresh = await backend.projectStatus(this.id)
    } catch {
      return
    }
    this.info = fresh
    const backendRunning = infoRunningSessionIds(fresh)
    // Runs the app missed are added; a session the backend no longer runs
    // and whose chat is not running either was a stale lock.
    const next = new Set<string>()
    for (const id of this.runningSessionIds) {
      const chat = this.chats.get(id)
      if (backendRunning.has(id) || (chat !== undefined && chatStatusIsRunning(chat.status))) next.add(id)
    }
    for (const id of backendRunning) next.add(id)
    this.runningSessionIds = next
    this.touch()
  }

  /** Recent tasks and memories for the Project tab and the palette. */
  async loadExtras(): Promise<void> {
    this.extrasLoading = true
    this.touch()
    let failure: string | null = null
    try {
      this.tasks = await backend.tasksList(this.id, 50)
    } catch (error) {
      failure = errorText(error)
    }
    try {
      this.memories = await backend.memoryList(this.id, null, 50)
    } catch (error) {
      failure = failure ?? errorText(error)
    }
    this.extrasError = failure
    this.extrasLoading = false
    this.touch()
  }

  async refreshModels(force: boolean): Promise<void> {
    this.models.loading = true
    this.touch()
    try {
      const list = await backend.modelsList(this.id, force)
      this.models.apply(list)
    } catch (error) {
      this.models.error = errorText(error)
      if (this.models.current === null) this.models.current = this.info.model
    } finally {
      this.models.loading = false
      this.touch()
    }
  }

  async runDoctor(probe: boolean): Promise<void> {
    this.doctorRunning = true
    this.touch()
    try {
      this.doctor = await backend.doctorRun(this.id, probe)
      this.doctorError = null
    } catch (error) {
      this.doctorError = errorText(error)
    } finally {
      this.doctorRunning = false
      this.touch()
    }
  }

  // MARK: - Helpers

  /** `ISO8601DateFormatter().string(from: Date())`. */
  private static now(): string {
    return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
  }
}

/** `sessions.sort { $0.updatedDate > $1.updatedDate }` (stable, like Swift's sort on equal dates in practice). */
function stableSortByUpdated(list: SessionSummary[]): SessionSummary[] {
  return list
    .map((session, index) => ({ session, index, date: sessionUpdatedDate(session).getTime() }))
    .sort((a, b) => b.date - a.date || a.index - b.index)
    .map((entry) => entry.session)
}

function sameJSON(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (a === null || b === null) return false
  try {
    return JSON.stringify(a) === JSON.stringify(b)
  } catch {
    return false
  }
}

// MARK: - Starters

/// `StarterSuggestion.fallback(for:)` (Views/Chat/EmptyChatView.swift):
/// shown when the model is not connected or `suggest/starters` came back empty.
export function starterFallback(info: ProjectInfo): StarterSuggestion[] {
  const command = info.validation_commands[0]?.command
  return [
    { kind: 'explain', text: 'Explain how this project is structured' },
    { kind: 'fix', text: command !== undefined ? `Run ${command} and fix any failures` : 'Find and fix a bug' },
    { kind: 'review', text: (info.git?.dirty_files ?? 0) > 0 ? 'Review my uncommitted changes' : 'Review the most recent commit' },
  ]
}

// MARK: - FileTreeModel

/** `FileNode` (Views/Inspector/FilesTab.swift): one entry of the Files tab's tree. */
export interface FileNode {
  /** Path relative to the project root, `/`-separated. */
  path: string
  name: string
  isDirectory: boolean
}

/** One row of `FileTreeModel.visibleRows`. */
export interface FileTreeRow {
  node: FileNode
  depth: number
}

/** `fs_list_dir`'s entries. */
interface DirEntryInfo {
  name: string
  path: string
  is_dir: boolean
  size: number
}

/// `FileTreeModel` (Views/Inspector/FilesTab.swift): lazily loaded directory
/// listings of one project root, read from disk one folder at a time as it
/// is expanded (`fs_list_dir`: `.git` and `.DS_Store` hidden, folders first,
/// then names in Finder order). Every change calls `onChange` (the project's
/// `touch`) and bumps `version`.
export class FileTreeModel {
  readonly root: string
  /** Relative directory path ("" for the root) → its sorted entries. */
  children = new Map<string, FileNode[]>()
  expanded = new Set<string>()
  selectedPath: string | null = null
  version = 0

  private readonly onChange: () => void
  private readonly loading = new Map<string, Promise<void>>()

  constructor(root: string, onChange: () => void = () => undefined) {
    this.root = root
    this.onChange = onChange
  }

  /** Whether `path`'s listing has arrived. */
  isLoaded(path: string): boolean {
    return this.children.has(path)
  }

  toggle(node: FileNode): void {
    if (!node.isDirectory) return
    const expanded = new Set(this.expanded)
    if (expanded.has(node.path)) {
      expanded.delete(node.path)
    } else {
      if (!this.children.has(node.path)) void this.load(node.path)
      expanded.add(node.path)
    }
    this.expanded = expanded
    this.changed()
  }

  select(path: string | null): void {
    if (path === this.selectedPath) return
    this.selectedPath = path
    this.changed()
  }

  collapseAll(): void {
    this.expanded = new Set()
    this.changed()
  }

  /// Re-reads the root and every folder listed so far (after git status
  /// changes or the refresh button); folders that vanished are dropped.
  async reload(): Promise<void> {
    const listed = [...new Set([...this.children.keys(), ''])].sort()
    const fresh = new Map<string, FileNode[]>()
    // Parents sort before their children, so each level is checked against
    // the fresh listing of the level above it.
    for (const path of listed) {
      if (path.length > 0) {
        const parent = FileTreeModel.parent(path)
        if (!(fresh.get(parent)?.some((node) => node.path === path) ?? false)) continue
      }
      fresh.set(path, await this.list(path))
    }
    this.children = fresh
    this.expanded = new Set([...this.expanded].filter((path) => fresh.has(path)))
    this.changed()
  }

  loadRootIfNeeded(): void {
    if (!this.children.has('')) void this.load('')
  }

  /** Rows in display order: each folder's entries follow it while expanded. */
  get visibleRows(): FileTreeRow[] {
    const rows: FileTreeRow[] = []
    const walk = (path: string, depth: number) => {
      for (const node of this.children.get(path) ?? []) {
        rows.push({ node, depth })
        if (node.isDirectory && this.expanded.has(node.path)) walk(node.path, depth + 1)
      }
    }
    walk('', 0)
    return rows
  }

  /** The absolute path of a relative tree path. */
  absolutePath(path: string): string {
    if (path.length === 0) return this.root
    const separator = this.root.includes('\\') && !this.root.includes('/') ? '\\' : '/'
    const base = this.root.endsWith('/') || this.root.endsWith('\\') ? this.root.slice(0, -1) : this.root
    return base + separator + path.split('/').join(separator)
  }

  private load(path: string): Promise<void> {
    const inFlight = this.loading.get(path)
    if (inFlight) return inFlight
    const task = (async () => {
      const nodes = await this.list(path)
      const children = new Map(this.children)
      children.set(path, nodes)
      this.children = children
      this.loading.delete(path)
      this.changed()
    })()
    this.loading.set(path, task)
    return task
  }

  private async list(path: string): Promise<FileNode[]> {
    try {
      const entries = await invoke<DirEntryInfo[]>('fs_list_dir', { path: this.absolutePath(path) })
      return entries.map((entry) => ({
        path: path.length === 0 ? entry.name : `${path}/${entry.name}`,
        name: entry.name,
        isDirectory: entry.is_dir,
      }))
    } catch {
      return []
    }
  }

  private changed(): void {
    this.version += 1
    this.onChange()
  }

  private static parent(path: string): string {
    const slash = path.lastIndexOf('/')
    return slash < 0 ? '' : path.slice(0, slash)
  }
}
