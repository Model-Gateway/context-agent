// Port of apps/macos/ContextAgent/App/TerminalSession.swift: the Inspector
// Terminal tab's shells. `openTerminal` is the thin typed wrapper over the
// Rust `terminal_*` commands and the `terminal:data` / `terminal:exit`
// events (src-tauri/src/terminal.rs, portable-pty: a pty on macOS and Linux,
// ConPTY on Windows). `TerminalSession` and `TerminalWorkspace` are the
// Swift classes of the same names: one login shell per tab in the project
// root, a strip of tabs per project, and a shell that exits closes its tab.
// The xterm.js view itself belongs to the Terminal tab; it keeps its
// `Terminal` on `TerminalSession.view` so switching tabs keeps scrollback and
// running programs, as the SwiftTerm view lives as long as its session.

import { useCallback, useSyncExternalStore } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { errorText } from '../backend/errors'
import { debug } from '../backend/log'

// MARK: - Wire types

/** `terminal_open`'s result. */
export interface TerminalOpened {
  id: string
  /** The shell's name for the tab title ("zsh", "pwsh"). */
  shell: string
  pid: number | null
}

/** A terminal's size in character cells. */
export interface TerminalSize {
  cols: number
  rows: number
}

interface DataPayload {
  id: string
  data: string
}

interface ExitPayload {
  id: string
  code: number | null
}

const DATA_EVENT = 'terminal:data'
const EXIT_EVENT = 'terminal:exit'
/** Output kept for a listener that has not attached yet (bytes of UTF-16, roughly). */
const MAX_BACKLOG = 1024 * 1024

// MARK: - TerminalProcess (the thin wrapper)

/**
 * One shell on a pseudo-terminal in the Rust shell (what SwiftTerm's
 * `LocalProcess` is to `LocalProcessTerminalView`). Output that arrives
 * before the first `onData` listener is kept and handed to it.
 */
export interface TerminalProcess {
  readonly id: string
  readonly shell: string
  readonly pid: number | null
  readonly exited: boolean
  /** The shell's exit status once it has exited (null when unknown). */
  readonly exitCode: number | null
  /** Subscribes to output (UTF-8 decoded text for `Terminal.write`). */
  onData(listener: (data: string) => void): () => void
  /** Called once when the shell has exited (at once when it already has). */
  onExit(listener: (code: number | null) => void): () => void
  /** Keystrokes and pastes. */
  write(data: string): Promise<void>
  resize(cols: number, rows: number): Promise<void>
  /** Hangs up and kills the shell; `onExit` follows. */
  close(): Promise<void>
}

/** Live processes by Rust session id. */
const processes = new Map<string, ProcessHandle>()
/** Events for sessions whose `terminal_open` has not returned yet. */
const early = new Map<string, { data: string[]; size: number; exit?: { code: number | null } }>()
let openingCount = 0
let listening: Promise<void> | null = null

function ensureListening(): Promise<void> {
  if (!listening) {
    listening = Promise.all([
      listen<DataPayload>(DATA_EVENT, (event) => route(event.payload.id, (process) => process.receive(event.payload.data), (entry) => {
        if (entry.size >= MAX_BACKLOG) return
        entry.data.push(event.payload.data)
        entry.size += event.payload.data.length
      })),
      listen<ExitPayload>(EXIT_EVENT, (event) => route(event.payload.id, (process) => process.finish(event.payload.code), (entry) => {
        entry.exit = { code: event.payload.code }
      })),
    ])
      .then(() => undefined)
      .catch((error: unknown) => {
        listening = null
        throw error
      })
  }
  return listening
}

function route(
  id: string,
  deliver: (process: ProcessHandle) => void,
  buffer: (entry: { data: string[]; size: number; exit?: { code: number | null } }) => void,
): void {
  const process = processes.get(id)
  if (process) {
    deliver(process)
    return
  }
  // Only hold on to output while an open is in flight: it belongs to that session.
  if (openingCount === 0) return
  let entry = early.get(id)
  if (!entry) {
    entry = { data: [], size: 0 }
    early.set(id, entry)
  }
  buffer(entry)
}

class ProcessHandle implements TerminalProcess {
  readonly id: string
  readonly shell: string
  readonly pid: number | null
  exited = false
  exitCode: number | null = null
  private readonly dataListeners = new Set<(data: string) => void>()
  private readonly exitListeners = new Set<(code: number | null) => void>()
  private backlog: string[] = []
  private backlogSize = 0

  constructor(opened: TerminalOpened) {
    this.id = opened.id
    this.shell = opened.shell
    this.pid = opened.pid
  }

  receive(data: string): void {
    if (this.dataListeners.size === 0) {
      this.backlog.push(data)
      this.backlogSize += data.length
      while (this.backlogSize > MAX_BACKLOG && this.backlog.length > 1) {
        this.backlogSize -= this.backlog.shift()?.length ?? 0
      }
      return
    }
    for (const listener of this.dataListeners) listener(data)
  }

  finish(code: number | null): void {
    if (this.exited) return
    this.exited = true
    this.exitCode = code
    processes.delete(this.id)
    for (const listener of [...this.exitListeners]) listener(code)
    this.exitListeners.clear()
  }

  onData(listener: (data: string) => void): () => void {
    this.dataListeners.add(listener)
    if (this.backlog.length > 0) {
      const backlog = this.backlog.join('')
      this.backlog = []
      this.backlogSize = 0
      listener(backlog)
    }
    return () => {
      this.dataListeners.delete(listener)
    }
  }

  onExit(listener: (code: number | null) => void): () => void {
    if (this.exited) {
      const code = this.exitCode
      queueMicrotask(() => listener(code))
      return () => undefined
    }
    this.exitListeners.add(listener)
    return () => {
      this.exitListeners.delete(listener)
    }
  }

  async write(data: string): Promise<void> {
    if (this.exited || data.length === 0) return
    try {
      await invoke('terminal_write', { id: this.id, data })
    } catch (error) {
      debug(() => `terminal_write failed: ${errorText(error)}`)
    }
  }

  async resize(cols: number, rows: number): Promise<void> {
    if (this.exited || !Number.isFinite(cols) || !Number.isFinite(rows) || cols < 1 || rows < 1) return
    try {
      await invoke('terminal_resize', { id: this.id, cols: Math.floor(cols), rows: Math.floor(rows) })
    } catch (error) {
      debug(() => `terminal_resize failed: ${errorText(error)}`)
    }
  }

  async close(): Promise<void> {
    if (this.exited) return
    try {
      await invoke('terminal_close', { id: this.id })
    } catch (error) {
      debug(() => `terminal_close failed: ${errorText(error)}`)
    }
  }
}

/**
 * Starts the user's shell in `cwd` (`terminal_open`): the login shell on
 * macOS and Linux, PowerShell on Windows. Rejects with a presentable
 * message when the shell cannot start.
 */
export async function openTerminal(cwd: string, size?: TerminalSize): Promise<TerminalProcess> {
  await ensureListening()
  openingCount += 1
  try {
    const opened = await invoke<TerminalOpened>('terminal_open', {
      cwd,
      cols: size ? Math.max(1, Math.floor(size.cols)) : null,
      rows: size ? Math.max(1, Math.floor(size.rows)) : null,
    })
    const process = new ProcessHandle(opened)
    processes.set(opened.id, process)
    const pending = early.get(opened.id)
    early.delete(opened.id)
    if (pending) {
      for (const data of pending.data) process.receive(data)
      if (pending.exit) process.finish(pending.exit.code)
    }
    return process
  } catch (error) {
    throw new Error(errorText(error))
  } finally {
    openingCount -= 1
    if (openingCount === 0) early.clear()
  }
}

// MARK: - TerminalSession

/** What a view of a session reads (`useTerminalSession`). */
export interface TerminalSessionSnapshot {
  id: string
  displayTitle: string
  title: string | null
  currentDirectory: string | null
  isRunning: boolean
  /** Why the shell could not start, shown in the view. */
  error: string | null
  version: number
}

let nextSessionNumber = 0

/**
 * `TerminalSession`: one shell in the Inspector Terminal tab, started in
 * the project root. Created synchronously like the Swift class: input and
 * size given before the shell is up are applied once it is.
 */
export class TerminalSession {
  readonly id: string
  /** "zsh 2" until the shell sets a title (OSC 0/2). */
  readonly number: number
  title: string | null = null
  currentDirectory: string | null
  isRunning = true
  error: string | null = null
  /** The shell's name once it has started ("zsh", "pwsh"). */
  shell: string | null = null
  /** Called once the shell exits (also after `terminate()`). */
  onExit: ((session: TerminalSession) => void) | undefined
  /**
   * The view's own object for this session (the xterm.js `Terminal`), kept
   * as long as the session so tab switches keep scrollback.
   */
  view: unknown = null

  private process: TerminalProcess | null = null
  private readonly dataListeners = new Set<(data: string) => void>()
  private backlog: string[] = []
  private backlogSize = 0
  private pendingInput: string[] = []
  private size: TerminalSize | null = null
  private terminateRequested = false
  private readonly changeListeners = new Set<() => void>()
  private version = 0
  private snapshotCache: TerminalSessionSnapshot | null = null

  constructor(directory: string, number: number, size?: TerminalSize) {
    nextSessionNumber += 1
    this.id = `terminal-${nextSessionNumber}`
    this.number = number
    this.currentDirectory = directory
    this.size = size ?? null
    void this.start(directory)
  }

  private async start(directory: string): Promise<void> {
    let process: TerminalProcess
    try {
      process = await openTerminal(directory, this.size ?? undefined)
    } catch (error) {
      this.error = errorText(error)
      this.isRunning = false
      this.deliver(`\r\n${this.error}\r\n`)
      this.changed()
      return
    }
    this.process = process
    this.shell = process.shell
    process.onData((data) => this.deliver(data))
    process.onExit(() => this.exited())
    if (this.terminateRequested) {
      void process.close()
    } else {
      if (this.size) void process.resize(this.size.cols, this.size.rows)
      for (const input of this.pendingInput) void process.write(input)
    }
    this.pendingInput = []
    this.changed()
  }

  /** "zsh", "zsh 2", … (`fallbackTitle`). */
  get fallbackTitle(): string {
    const name = this.shell ?? 'shell'
    return this.number > 1 ? `${name} ${this.number}` : name
  }

  get displayTitle(): string {
    return this.title ?? this.fallbackTitle
  }

  /** Subscribes the view to output; output from before it attached comes first. */
  onData(listener: (data: string) => void): () => void {
    this.dataListeners.add(listener)
    if (this.backlog.length > 0) {
      const backlog = this.backlog.join('')
      this.backlog = []
      this.backlogSize = 0
      listener(backlog)
    }
    return () => {
      this.dataListeners.delete(listener)
    }
  }

  /** Keystrokes from the view. */
  write(data: string): void {
    if (!this.isRunning) return
    if (this.process) void this.process.write(data)
    else this.pendingInput.push(data)
  }

  /** The view's size in cells. */
  resize(cols: number, rows: number): void {
    if (!Number.isFinite(cols) || !Number.isFinite(rows) || cols < 1 || rows < 1) return
    if (this.size && this.size.cols === cols && this.size.rows === rows) return
    this.size = { cols, rows }
    if (this.process) void this.process.resize(cols, rows)
  }

  /** `run(_:)`: types `command` and Return; the pty buffers it until the shell reads. */
  run(command: string): void {
    this.write(`${command}\r`)
  }

  /** `terminate()`. */
  terminate(): void {
    if (!this.isRunning) return
    this.isRunning = false
    this.terminateRequested = true
    if (this.process) void this.process.close()
    this.changed()
  }

  /** `titleChanged(_:)`: from the view's OSC 0/2 handler. */
  titleChanged(title: string): void {
    const trimmed = title.trim()
    const next = trimmed.length === 0 ? null : trimmed
    if (next === this.title) return
    this.title = next
    this.changed()
  }

  /** `directoryChanged(_:)`: OSC 7 reports a file:// URL. */
  directoryChanged(directory: string | null): void {
    if (directory === null) return
    let path = directory
    try {
      const url = new URL(directory)
      if (url.protocol === 'file:') path = decodeURIComponent(url.pathname)
    } catch {
      // Not a URL: a plain path.
    }
    if (path === this.currentDirectory) return
    this.currentDirectory = path
    this.changed()
  }

  /** Change notifications (title, running state, shell name). */
  subscribe(listener: () => void): () => void {
    this.changeListeners.add(listener)
    return () => {
      this.changeListeners.delete(listener)
    }
  }

  getSnapshot(): TerminalSessionSnapshot {
    if (!this.snapshotCache) {
      this.snapshotCache = {
        id: this.id,
        displayTitle: this.displayTitle,
        title: this.title,
        currentDirectory: this.currentDirectory,
        isRunning: this.isRunning,
        error: this.error,
        version: this.version,
      }
    }
    return this.snapshotCache
  }

  private deliver(data: string): void {
    if (this.dataListeners.size === 0) {
      this.backlog.push(data)
      this.backlogSize += data.length
      while (this.backlogSize > MAX_BACKLOG && this.backlog.length > 1) {
        this.backlogSize -= this.backlog.shift()?.length ?? 0
      }
      return
    }
    for (const listener of this.dataListeners) listener(data)
  }

  private exited(): void {
    this.isRunning = false
    const onExit = this.onExit
    this.onExit = undefined
    onExit?.(this)
    this.changed()
  }

  private changed(): void {
    this.version += 1
    this.snapshotCache = null
    for (const listener of [...this.changeListeners]) listener()
  }
}

// MARK: - TerminalWorkspace

/** What the tab strip reads (`useTerminalWorkspace`). */
export interface TerminalWorkspaceSnapshot {
  directory: string
  sessions: readonly TerminalSession[]
  selectedId: string | null
  selected: TerminalSession | null
  version: number
}

/**
 * `TerminalWorkspace`: a project's terminal tabs, the sessions in tab order
 * and the selected one. A shell that exits closes its tab.
 */
export class TerminalWorkspace {
  readonly directory: string
  sessions: TerminalSession[] = []
  selectedId: string | null = null
  private opened = 0
  private readonly listeners = new Set<() => void>()
  private readonly sessionSubscriptions = new Map<string, () => void>()
  private version = 0
  private snapshotCache: TerminalWorkspaceSnapshot | null = null

  constructor(directory: string) {
    this.directory = directory
  }

  get selected(): TerminalSession | null {
    return this.sessions.find((session) => session.id === this.selectedId) ?? this.sessions.at(-1) ?? null
  }

  /** Opens the first tab the first time the Terminal tab shows. */
  startIfNeeded(size?: TerminalSize): void {
    if (this.opened !== 0) return
    this.newSession(undefined, size)
  }

  /** `newSession(running:)`: a new tab, selected; `command` is typed into it. */
  newSession(command?: string, size?: TerminalSize): TerminalSession {
    this.opened += 1
    const session = new TerminalSession(this.directory, this.opened, size)
    if (command !== undefined) session.run(command)
    session.onExit = (exited) => this.remove(exited)
    this.sessionSubscriptions.set(session.id, session.subscribe(() => this.changed()))
    this.sessions = [...this.sessions, session]
    this.selectedId = session.id
    this.changed()
    return session
  }

  select(id: string): void {
    if (this.selectedId === id || !this.sessions.some((session) => session.id === id)) return
    this.selectedId = id
    this.changed()
  }

  close(session: TerminalSession): void {
    session.terminate()
    this.remove(session)
  }

  /** Hangs every shell up (the project closed). */
  terminateAll(): void {
    for (const session of this.sessions) {
      session.onExit = undefined
      session.terminate()
    }
    for (const unsubscribe of this.sessionSubscriptions.values()) unsubscribe()
    this.sessionSubscriptions.clear()
    this.sessions = []
    this.selectedId = null
    this.changed()
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  getSnapshot(): TerminalWorkspaceSnapshot {
    if (!this.snapshotCache) {
      this.snapshotCache = {
        directory: this.directory,
        sessions: this.sessions,
        selectedId: this.selectedId,
        selected: this.selected,
        version: this.version,
      }
    }
    return this.snapshotCache
  }

  private remove(session: TerminalSession): void {
    const index = this.sessions.findIndex((candidate) => candidate.id === session.id)
    if (index < 0) return
    this.sessionSubscriptions.get(session.id)?.()
    this.sessionSubscriptions.delete(session.id)
    const sessions = this.sessions.filter((candidate) => candidate.id !== session.id)
    this.sessions = sessions
    if (this.selectedId === session.id) {
      this.selectedId = sessions.length === 0 ? null : (sessions[Math.min(index, sessions.length - 1)]?.id ?? null)
    }
    this.changed()
  }

  private changed(): void {
    this.version += 1
    this.snapshotCache = null
    for (const listener of [...this.listeners]) listener()
  }
}

// MARK: - Hooks

/** Re-renders with the workspace's tabs and selection; null for no workspace. */
export function useTerminalWorkspace(workspace: TerminalWorkspace | null): TerminalWorkspaceSnapshot | null {
  const subscribe = useCallback(
    (listener: () => void) => (workspace ? workspace.subscribe(listener) : () => undefined),
    [workspace],
  )
  return useSyncExternalStore(subscribe, () => (workspace ? workspace.getSnapshot() : null))
}

/** Re-renders with one session's title and state; null for no session. */
export function useTerminalSession(session: TerminalSession | null): TerminalSessionSnapshot | null {
  const subscribe = useCallback(
    (listener: () => void) => (session ? session.subscribe(listener) : () => undefined),
    [session],
  )
  return useSyncExternalStore(subscribe, () => (session ? session.getSnapshot() : null))
}
