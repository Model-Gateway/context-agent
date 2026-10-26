// Port of apps/macos/ContextAgent/App/BrowserWorkspace.swift: the Inspector
// Browser tab's page for one project. The page itself is a Tauri child
// webview owned by the Rust shell (src-tauri/src/browser.rs) and laid over
// the tab's area of the main window; this class is the observable side the
// toolbar reads (address, title, loading, progress, back / forward, the
// last load error, what the agent is doing) and the typed wrapper over the
// `browser_*` commands and the `browser:state` event.
//
// The page outlives tab switches: `hide()` parks it out of the window (it
// keeps loading and running script), and the agent's `browser/request`
// actions (`perform`) run in it whether or not the tab is shown — the
// Inspector never switches to the Browser tab for the agent. A native
// webview always paints above the HTML, so the page is only placed in the
// window while it has something to show (a page, or one loading); before
// that the tab's own placeholder shows through.

import { useCallback, useSyncExternalStore } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { errorText } from '../backend/errors'
import { debug } from '../backend/log'
import { jsonGet, jsonInt, jsonString, type BrowserReply, type JSONValue } from '../backend/types'

// MARK: - Wire types

/** The page's area in the main window, in CSS pixels. */
export interface BrowserBounds {
  x: number
  y: number
  width: number
  height: number
}

/** The `browser:state` payload / `browser_state` result (Rust `BrowserState`). */
export interface BrowserPageState {
  project_id: string
  /** Null before the first page (the blank start page). */
  url: string | null
  title: string
  loading: boolean
  can_go_back: boolean
  can_go_forward: boolean
  /** 0…1 while loading. */
  progress: number
  load_error: string | null
  /** Whether the page is placed in the window. */
  visible: boolean
}

interface BrowserReplyWire {
  ok: boolean
  content: string
  url: string | null
  title: string | null
  error: string | null
}

/** What a view of the page reads (`useBrowserWorkspace`). */
export interface BrowserSnapshot {
  projectId: string
  url: string | null
  title: string
  isLoading: boolean
  progress: number
  canGoBack: boolean
  canGoForward: boolean
  loadError: string | null
  agentActivity: string | null
  recentAgentAction: string | null
  addressText: string
  isAgentDriving: boolean
  hasPage: boolean
  version: number
}

const STATE_EVENT = 'browser:state'
/** How long the agent's last action stays on the status line. */
const RECENT_ACTION_MS = 4000
/** Re-measure for this long after a transition or animation starts. */
const ANIMATION_BURST_MS = 400

/** Open workspaces by project id, for routing `browser:state`. */
const workspaces = new Map<string, BrowserWorkspace>()
let listening: Promise<void> | null = null

function ensureListening(): Promise<void> {
  if (!listening) {
    listening = listen<BrowserPageState>(STATE_EVENT, (event) => {
      workspaces.get(event.payload.project_id)?.apply(event.payload)
    })
      .then(() => undefined)
      .catch((error: unknown) => {
        listening = null
        debug(() => `could not listen for ${STATE_EVENT}: ${errorText(error)}`)
      })
  }
  return listening
}

function sameBounds(a: BrowserBounds | null, b: BrowserBounds | null): boolean {
  if (a === null || b === null) return a === b
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height
}

// MARK: - BrowserWorkspace

/**
 * `BrowserWorkspace`: one project's long-lived page, browsed by the user
 * and driven by the agent. Create one per project (`AppState.browser(for:)`)
 * and `destroy()` it when the project closes (`tearDown()`).
 */
export class BrowserWorkspace {
  static readonly startPage = 'about:blank'

  readonly projectId: string
  url: string | null = null
  title = ''
  isLoading = false
  progress = 0
  canGoBack = false
  canGoForward = false
  /** The last navigation failure, shown under the toolbar until the next load. */
  loadError: string | null = null
  /** What the agent is doing right now ("Opening example.com…"); null when idle. */
  agentActivity: string | null = null
  /** The agent's last finished action, kept for a few seconds. */
  recentAgentAction: string | null = null
  /** The address field's text; follows the page, edited by the user. */
  addressText = ''

  /** Serializes the agent's actions (two sessions may drive one browser). */
  private tail: Promise<unknown> = Promise.resolve()
  private recentActionReset: ReturnType<typeof setTimeout> | null = null
  private bounds: BrowserBounds | null = null
  private wantsVisible = false
  /** What the Rust side was last told: placed in the window or not. */
  private placed = false
  private placement: Promise<void> = Promise.resolve()
  private destroyed = false
  private readonly listeners = new Set<() => void>()
  private version = 0
  private snapshotCache: BrowserSnapshot | null = null

  constructor(projectId: string) {
    this.projectId = projectId
    workspaces.set(projectId, this)
    void ensureListening().then(() => this.hydrate())
  }

  get isAgentDriving(): boolean {
    return this.agentActivity !== null
  }

  get hasPage(): boolean {
    return this.url !== null && this.url !== BrowserWorkspace.startPage
  }

  // MARK: Observation

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  getSnapshot(): BrowserSnapshot {
    if (!this.snapshotCache) {
      this.snapshotCache = {
        projectId: this.projectId,
        url: this.url,
        title: this.title,
        isLoading: this.isLoading,
        progress: this.progress,
        canGoBack: this.canGoBack,
        canGoForward: this.canGoForward,
        loadError: this.loadError,
        agentActivity: this.agentActivity,
        recentAgentAction: this.recentAgentAction,
        addressText: this.addressText,
        isAgentDriving: this.isAgentDriving,
        hasPage: this.hasPage,
        version: this.version,
      }
    }
    return this.snapshotCache
  }

  /** The address field's binding. */
  setAddressText(text: string): void {
    if (text === this.addressText) return
    this.addressText = text
    this.changed()
  }

  /** A `browser:state` from the Rust side (KVO on the WKWebView in Swift). */
  apply(state: BrowserPageState): void {
    if (this.destroyed) return
    const urlChanged = state.url !== this.url
    this.url = state.url
    this.title = state.title
    this.isLoading = state.loading
    this.progress = state.progress
    this.canGoBack = state.can_go_back
    this.canGoForward = state.can_go_forward
    this.loadError = state.load_error
    // `pageChanged(url:)`: the field follows the page.
    if (urlChanged && state.url !== null && state.url !== BrowserWorkspace.startPage) {
      this.addressText = state.url
    }
    this.changed()
    this.syncPlacement()
  }

  private async hydrate(): Promise<void> {
    try {
      const state = await invoke<BrowserPageState | null>('browser_state', { project_id: this.projectId })
      if (state) this.apply(state)
    } catch (error) {
      debug(() => `browser_state failed: ${errorText(error)}`)
    }
  }

  // MARK: Placement

  /** The tab's area changed (or is gone: null). */
  setBounds(bounds: BrowserBounds | null): void {
    const next = bounds && bounds.width >= 1 && bounds.height >= 1
      ? { x: Math.round(bounds.x), y: Math.round(bounds.y), width: Math.round(bounds.width), height: Math.round(bounds.height) }
      : null
    if (sameBounds(next, this.bounds)) return
    this.bounds = next
    if (next && this.placed) {
      this.enqueue(() => invoke('browser_set_bounds', { project_id: this.projectId, bounds: next }))
    }
    this.syncPlacement()
  }

  /** The Browser tab shows this page. */
  show(): void {
    this.wantsVisible = true
    this.syncPlacement()
  }

  /** Another tab (or project) is shown: the page is parked and keeps running. */
  hide(): void {
    this.wantsVisible = false
    this.syncPlacement()
  }

  /**
   * Lays the page over `element` and keeps it there (resizes, window
   * moves, the Inspector's animations) until the returned function is
   * called, which hides it again.
   */
  track(element: HTMLElement): () => void {
    let frame = 0
    let burstUntil = 0
    const measure = (): void => {
      frame = 0
      if (!element.isConnected) {
        this.setBounds(null)
        return
      }
      const rect = element.getBoundingClientRect()
      this.setBounds({ x: rect.left, y: rect.top, width: rect.width, height: rect.height })
      if (performance.now() < burstUntil) frame = requestAnimationFrame(measure)
    }
    const schedule = (burstMs: number): void => {
      burstUntil = Math.max(burstUntil, performance.now() + burstMs)
      if (frame === 0) frame = requestAnimationFrame(measure)
    }
    const onResize = (): void => schedule(0)
    const onAnimation = (): void => schedule(ANIMATION_BURST_MS)
    const observer = new ResizeObserver(onResize)
    observer.observe(element)
    observer.observe(document.documentElement)
    window.addEventListener('resize', onResize)
    const animationEvents = ['transitionrun', 'transitionend', 'animationstart', 'animationend'] as const
    for (const name of animationEvents) document.addEventListener(name, onAnimation, true)
    measure()
    this.show()
    schedule(ANIMATION_BURST_MS)
    return () => {
      if (frame !== 0) cancelAnimationFrame(frame)
      frame = 0
      observer.disconnect()
      window.removeEventListener('resize', onResize)
      for (const name of animationEvents) document.removeEventListener(name, onAnimation, true)
      this.hide()
    }
  }

  /**
   * Places the page in the window while the tab shows it and it has
   * something to show; parks it otherwise.
   */
  private syncPlacement(): void {
    if (this.destroyed) return
    const bounds = this.bounds
    const shouldPlace = this.wantsVisible && bounds !== null && (this.hasPage || this.isLoading)
    if (shouldPlace === this.placed) return
    this.placed = shouldPlace
    if (shouldPlace && bounds) {
      this.enqueue(() => invoke('browser_create', { project_id: this.projectId, bounds, visible: true }))
    } else {
      this.enqueue(() => invoke('browser_hide', { project_id: this.projectId }))
    }
  }

  /** Runs placement commands in order. */
  private enqueue(command: () => Promise<unknown>): void {
    this.placement = this.placement
      .then(async () => {
        if (this.destroyed) return
        await command()
      })
      .catch((error: unknown) => {
        debug(() => `browser placement failed: ${errorText(error)}`)
      })
  }

  // MARK: User navigation

  /**
   * `open(_:)`: loads what the user typed — an address (scheme added when
   * missing), else a DuckDuckGo search for the words.
   */
  navigate(text: string): Promise<void> {
    const trimmed = text.trim()
    if (trimmed.length === 0) return Promise.resolve()
    if (this.loadError !== null) {
      this.loadError = null
      this.changed()
    }
    return this.command('browser_navigate', { url: trimmed })
  }

  /** `open(_:)` under its Swift name. */
  open(text: string): Promise<void> {
    return this.navigate(text)
  }

  back(): Promise<void> {
    return this.command('browser_back')
  }

  forward(): Promise<void> {
    return this.command('browser_forward')
  }

  /** `goBack()`. */
  goBack(): Promise<void> {
    return this.back()
  }

  /** `goForward()`. */
  goForward(): Promise<void> {
    return this.forward()
  }

  reload(): Promise<void> {
    if (this.loadError !== null) {
      this.loadError = null
      this.changed()
    }
    return this.command('browser_reload')
  }

  /** `stop()`: stops loading. */
  stop(): Promise<void> {
    return this.command('browser_stop')
  }

  private async command(name: string, extra: Record<string, unknown> = {}): Promise<void> {
    if (this.destroyed) return
    try {
      await invoke(name, { project_id: this.projectId, ...extra })
    } catch (error) {
      const message = errorText(error)
      debug(() => `${name} failed: ${message}`)
      this.loadError = message
      this.changed()
    }
  }

  /** `tearDown()`: lets the page go when its project closes. */
  destroy(): Promise<void> {
    if (this.destroyed) return Promise.resolve()
    this.destroyed = true
    if (this.recentActionReset !== null) clearTimeout(this.recentActionReset)
    this.recentActionReset = null
    if (workspaces.get(this.projectId) === this) workspaces.delete(this.projectId)
    this.listeners.clear()
    return invoke('browser_destroy', { project_id: this.projectId })
      .then(() => undefined)
      .catch((error: unknown) => {
        debug(() => `browser_destroy failed: ${errorText(error)}`)
      })
  }

  /** `tearDown()` under its Swift name. */
  tearDown(): Promise<void> {
    return this.destroy()
  }

  // MARK: Agent actions

  /**
   * `perform(_:)`: one `browser/request` action (`action` is the request's
   * `action` object), answered with the reply for `browser/respond`.
   * Actions run one after another; the tab is never switched.
   */
  perform(action: JSONValue): Promise<BrowserReply> {
    const run = (): Promise<BrowserReply> => this.run(action)
    const task = this.tail.then(run, run)
    this.tail = task.catch(() => undefined)
    return task
  }

  private async run(action: JSONValue): Promise<BrowserReply> {
    if (this.destroyed) return { ok: false, content: '', url: this.url, title: this.title, error: 'the browser is gone' }
    this.agentActivity = BrowserWorkspace.activityLabel(action)
    this.changed()
    try {
      const reply = await invoke<BrowserReplyWire>('browser_perform', { project_id: this.projectId, action })
      return {
        ok: reply.ok,
        content: reply.content,
        url: reply.url,
        title: reply.title,
        error: reply.error,
      }
    } catch (error) {
      return { ok: false, content: '', url: this.url, title: this.title, error: errorText(error) }
    } finally {
      this.recentAgentAction = this.agentActivity
      this.agentActivity = null
      if (this.recentActionReset !== null) clearTimeout(this.recentActionReset)
      this.recentActionReset = setTimeout(() => {
        this.recentActionReset = null
        this.recentAgentAction = null
        this.changed()
      }, RECENT_ACTION_MS)
      this.changed()
    }
  }

  /** `activityLabel(for:)`: "Opening example.com…", "Clicking [3]…". */
  static activityLabel(action: JSONValue): string {
    const kind = jsonString(jsonGet(action, 'action')) ?? 'browser'
    const ref = jsonInt(jsonGet(action, 'ref'))
    const element = ref !== null ? `[${ref}]` : 'an element'
    switch (kind) {
      case 'navigate': {
        const text = jsonString(jsonGet(action, 'url'))
        let host: string | null = null
        if (text !== null) {
          try {
            host = new URL(text).hostname || null
          } catch {
            host = null
          }
        }
        return `Opening ${host ?? text ?? 'a page'}…`
      }
      case 'back':
        return 'Going back…'
      case 'forward':
        return 'Going forward…'
      case 'reload':
        return 'Reloading…'
      case 'read':
        return 'Reading the page…'
      case 'snapshot':
        return "Listing the page's elements…"
      case 'click':
        return `Clicking ${element}…`
      case 'type':
        return `Typing into ${element}…`
      case 'select':
        return `Choosing an option in ${element}…`
      case 'scroll':
        return 'Scrolling…'
      case 'evaluate':
        return 'Running script in the page…'
      case 'screenshot':
        return 'Taking a screenshot…'
      default:
        return 'Driving the browser…'
    }
  }

  private changed(): void {
    this.version += 1
    this.snapshotCache = null
    for (const listener of [...this.listeners]) listener()
  }
}

// MARK: - Hooks

/** Re-renders with the page's state; null for no workspace. */
export function useBrowserWorkspace(workspace: BrowserWorkspace | null): BrowserSnapshot | null {
  const subscribe = useCallback(
    (listener: () => void) => (workspace ? workspace.subscribe(listener) : () => undefined),
    [workspace],
  )
  return useSyncExternalStore(subscribe, () => (workspace ? workspace.getSnapshot() : null))
}
