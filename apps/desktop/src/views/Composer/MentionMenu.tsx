// Port of apps/macos/ContextAgent/Views/Composer/MentionMenu.swift: `@`
// mentions in the composer, as in Claude Code and Codex. Typing `@` lists
// the installed apps (MCP servers, `mcp/status`) and the project's files
// (`files/search`); ↑/↓ choose, Return or Tab inserts `@name ` or `@path `
// (`@"path" ` when it has spaces), Esc hides the list. The backend reads the
// mentions from the sent text (docs/configuration.md "MCP servers and
// apps", "`@` mentions").
//
// `MentionBridge` sits between the composer's textarea (which finds the
// `@word` under the caret and forwards keys) and `MentionMenu` (which lists
// the matches and inserts the choice through the textarea, so it is one
// undoable edit). `mentionRanges` is what the composer tints in accent.

import { memo, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import type { McpCatalogApp, McpServerStatus } from '../../backend/types'
import { backend, useApp } from '../../state/app'
import { isReady } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { AppIcon } from '../Apps/AppIcon'
import styles from './MentionMenu.module.css'

// MARK: - MentionBridge

/** `MentionBridge.Query`: what follows the `@`, and `@` through the caret (UTF-16 offsets, `end` exclusive). */
export interface MentionQuery {
  text: string
  range: { start: number; end: number }
}

function sameQuery(a: MentionQuery | null, b: MentionQuery | null): boolean {
  if (a === null || b === null) return a === b
  return a.text === b.text && a.range.start === b.range.start && a.range.end === b.range.end
}

const WHITESPACE = /\s/
/** Opening brackets and quotes allowed before the `@`. */
const OPENERS = '([{"\'`'

/// `MentionBridge` (MentionMenu.swift): the `@word` under the caret, the
/// menu's selection and choices, and the textarea the choice is inserted
/// into. Views subscribe with `useMentionState(bridge)`.
export class MentionBridge {
  /** The `@word` being typed, if any. */
  query: MentionQuery | null = null
  selected = 0
  /** Bumped by ↑↓ only: the menu scrolls the selection into view for these, never for hover. */
  scrollRequest = 0
  /** What accepting each row inserts, in menu order (set by the menu). */
  choices: string[] = []
  /** An `@` the user dismissed with Esc stays quiet until a new one is typed. */
  dismissedLocation: number | null = null
  textarea: HTMLTextAreaElement | null = null

  private version = 0
  private listeners = new Set<() => void>()

  /** `useSyncExternalStore` plumbing. */
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  getVersion = (): number => this.version

  private emit(): void {
    this.version += 1
    this.listeners.forEach((listener) => listener())
  }

  get isOpen(): boolean {
    return this.query !== null && this.choices.length > 0
  }

  /// `update(from:)`: the `@word` ending at the caret: a word (no
  /// whitespace) that starts with `@`, after any opening brackets or quotes;
  /// `@"…` for a quoted path. What follows the `@` is the query. Nothing
  /// while text is selected or an IME composition is in progress.
  update(textarea: HTMLTextAreaElement, composing: boolean): void {
    const caret = textarea.selectionStart
    if (caret !== textarea.selectionEnd || composing) return this.set(null)
    const string = textarea.value
    let start = caret
    while (start > 0 && !WHITESPACE.test(string.charAt(start - 1))) start -= 1
    let at = start
    while (at < caret && OPENERS.includes(string.charAt(at))) at += 1
    // `@`, or the full-width `＠` input methods type.
    const lead = string.charAt(at)
    if (!(at < caret && (lead === '@' || lead === '＠'))) return this.set(null)
    if (this.dismissedLocation === at) return this.set(null)
    this.dismissedLocation = null
    let text = string.slice(at + 1, caret)
    if (text.startsWith('"')) text = text.slice(1)
    if (text.includes('@') || text.includes('＠')) return this.set(null)
    this.set({ text, range: { start: at, end: caret } })
  }

  private set(query: MentionQuery | null): void {
    if (sameQuery(this.query, query)) return
    if (this.query?.range.start !== query?.range.start) this.selected = 0
    this.query = query
    this.emit()
  }

  /** The menu's rows changed: what each inserts (keeps the selection in range). */
  setChoices(choices: string[]): void {
    const changed = choices.length !== this.choices.length || choices.some((choice, index) => choice !== this.choices[index])
    this.choices = choices
    if (this.selected >= choices.length && this.selected !== 0) {
      this.selected = 0
      this.emit()
    } else if (changed) {
      this.emit()
    }
  }

  setSelected(index: number): void {
    if (index === this.selected) return
    this.selected = index
    this.emit()
  }

  move(delta: number): void {
    const count = this.choices.length
    if (count === 0) return
    this.selected = (this.selected + delta + count) % count
    this.scrollRequest += 1
    this.emit()
  }

  acceptSelected(): void {
    const choice = this.choices[this.selected]
    if (choice !== undefined) this.accept(choice)
  }

  /// Replaces the `@word` with `insertion` (`@github `, `@src/a.rs `) as one
  /// undoable edit; the caret ends after it.
  accept(insertion: string): void {
    const textarea = this.textarea
    const query = this.query
    if (!textarea || !query) return
    textarea.focus()
    textarea.setSelectionRange(query.range.start, query.range.end)
    // `insertText` keeps the edit on the textarea's undo stack.
    const inserted = typeof document.execCommand === 'function' && document.execCommand('insertText', false, insertion)
    if (!inserted) {
      textarea.setRangeText(insertion, query.range.start, query.range.end, 'end')
      textarea.dispatchEvent(new Event('input', { bubbles: true }))
    }
    this.query = null
    this.emit()
  }

  dismiss(): void {
    this.dismissedLocation = this.query?.range.start ?? null
    this.query = null
    this.emit()
  }
}

/** Re-renders a view whenever the bridge changes; returns the bridge's version. */
export function useMentionState(bridge: MentionBridge): number {
  return useSyncExternalStore(bridge.subscribe, bridge.getVersion, bridge.getVersion)
}

// MARK: - Tinting

/** A character range to tint (UTF-16 offsets, `end` exclusive). */
export interface MentionRange {
  start: number
  end: number
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\/-]/g, '\\$&')
}

/** A `@` may follow the start, whitespace or an opening bracket / quote (Swift's look-behind). */
const LEAD = '(^|[\\s(\\[{"\'`])'
/** Paths: a quoted one, or a word with a `/` or `.` inside. */
const PATH_PATTERN = '@(?:"[^"\\n]+"|[^\\s@"]*[/.][^\\s@"]*[^\\s@".,;:!?)\\]}])'

/// `MentionBridge.highlight(in:)`: the `@name` of every installed app and
/// every `@path` in `text`, in order, merged where they overlap.
export function mentionRanges(text: string, installed: readonly string[]): MentionRange[] {
  if (!text.includes('@')) return []
  const patterns: string[] = []
  if (installed.length > 0) {
    patterns.push(`${LEAD}(@(?:${installed.map(escapeRegExp).join('|')})(?![A-Za-z0-9_-]))`)
  }
  patterns.push(`${LEAD}(${PATH_PATTERN})`)
  const ranges: MentionRange[] = []
  for (const pattern of patterns) {
    let regex: RegExp
    try {
      regex = new RegExp(pattern, 'gi')
    } catch {
      continue
    }
    for (const match of text.matchAll(regex)) {
      const lead = match[1] ?? ''
      const mention = match[2] ?? ''
      if (mention.length === 0 || match.index === undefined) continue
      const start = match.index + lead.length
      ranges.push({ start, end: start + mention.length })
    }
  }
  ranges.sort((a, b) => a.start - b.start)
  const merged: MentionRange[] = []
  for (const range of ranges) {
    const last = merged[merged.length - 1]
    if (last && range.start <= last.end) last.end = Math.max(last.end, range.end)
    else merged.push({ ...range })
  }
  return merged
}

// MARK: - Menu

/** `MentionMenu.Item`. */
interface MentionItem {
  kind: 'app' | 'file'
  /** The catalog entry of an app (logo, title), when the catalog has it. */
  app: McpCatalogApp | null
  /** App name or file path. */
  name: string
  title: string
  state: string
  tint: string
  /** What accepting the row inserts. */
  insertion: string
  id: string
}

/** `MentionMenu.split(_:)`: `src/app/main.rs` → (`src/app`, `main.rs`). */
function splitPath(path: string): { directory: string; name: string } {
  const slash = path.lastIndexOf('/')
  if (slash < 0) return { directory: '', name: path }
  return { directory: path.slice(0, slash), name: path.slice(slash + 1) }
}

/** `MentionMenu.insertion(forPath:)`: `@path ` or `@"path" ` (with spaces). */
function insertionForPath(path: string): string {
  return /\s/.test(path) ? `@"${path}" ` : `@${path} `
}

/** Letters, digits, `_` and `-` only: a name-like query. */
const NAME_LIKE = /^[\p{L}\p{N}_-]*$/u

/// `MentionMenu.apps(for:appState:)`: installed apps matching `query`: name
/// prefixes first, then names or titles containing it. Only for name-like
/// queries; an app name typed in full closes the app part (Return sends).
function appItems(query: string, servers: readonly McpServerStatus[], catalog: readonly McpCatalogApp[]): MentionItem[] {
  const q = query.toLowerCase()
  if (!NAME_LIKE.test(q)) return []
  const all: MentionItem[] = servers.map((status) => {
    const app = catalog.find((entry) => entry.id === (status.app ?? status.name)) ?? null
    let state: string
    let tint: string
    switch (status.state) {
      case 'connected': {
        const n = status.tools.filter((tool) => tool.enabled).length
        state = `${n} tool${n === 1 ? '' : 's'}`
        tint = 'var(--text-tertiary)'
        break
      }
      case 'connecting':
        state = 'Connecting'
        tint = 'var(--warning)'
        break
      case 'disabled':
        state = 'Paused'
        tint = 'var(--text-tertiary)'
        break
      case 'failed':
        state = 'Failed'
        tint = 'var(--danger)'
        break
    }
    return { kind: 'app', app, name: status.name, title: app?.name ?? status.name, state, tint, insertion: `@${status.name} `, id: `app:${status.name}` }
  })
  if (q.length > 0 && all.some((item) => item.name.toLowerCase() === q)) return []
  const prefix = all.filter((item) => item.name.toLowerCase().startsWith(q))
  const contains = all.filter(
    (item) => !item.name.toLowerCase().startsWith(q) && (item.name.toLowerCase().includes(q) || item.title.toLowerCase().includes(q)),
  )
  return [...prefix, ...contains]
}

/// `MentionMenu.files(_:query:)`: the backend's matches as rows; a path
/// already typed in full needs no menu.
function fileItems(paths: readonly string[], query: string): MentionItem[] {
  if (paths.length === 1 && paths[0] === query) return []
  return paths.map((path) => ({
    kind: 'file',
    app: null,
    name: path,
    title: path,
    state: '',
    tint: 'var(--text-tertiary)',
    insertion: insertionForPath(path),
    id: `file:${path}`,
  }))
}

export interface MentionMenuProps {
  bridge: MentionBridge
  projectId: string
}

/// `MentionMenu` (MentionMenu.swift): the list above the composer (its
/// bottom 8 px over the card's top-left corner): installed apps matching the
/// `@word` being typed, then the project's files; at most 10 rows, a
/// keyboard hint at the bottom. 380 px.
export function MentionMenu({ bridge, projectId }: MentionMenuProps) {
  useMentionState(bridge)
  const query = bridge.query?.text ?? null
  const servers = useApp((state) => state.mcpServers)
  const catalog = useApp((state) => state.mcpCatalog)
  const [files, setFiles] = useState<string[]>([])
  const menuRef = useRef<HTMLDivElement>(null)
  const itemsRef = useRef<HTMLDivElement>(null)
  /** The room above the card inside the pane that clips it (null: unmeasured). */
  const [maxHeight, setMaxHeight] = useState<number | null>(null)
  const scrolledFor = useRef(0)

  const items = query !== null ? [...appItems(query, servers, catalog), ...fileItems(files, query)].slice(0, 10) : []
  const choicesKey = items.map((item) => item.insertion).join('\u0000')

  useEffect(() => {
    bridge.setChoices(choicesKey.length === 0 ? [] : choicesKey.split('\u0000'))
  }, [bridge, choicesKey])

  useEffect(() => {
    if (query === null) {
      setFiles([])
      return
    }
    let cancelled = false
    // A short pause so fast typing sends one search, not one per key.
    const timer = window.setTimeout(() => {
      if (cancelled || !isReady(useApp.getState())) return
      void backend
        .filesSearch(projectId, query)
        .catch(() => [] as string[])
        .then((found) => {
          if (!cancelled) setFiles(found)
        })
    }, 60)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [query, projectId])

  // Ten rows are about 400 px. Swift's overlay is cut off by the pane
  // around the composer when the card sits higher than that (the empty
  // chat at the default window size); here the rows scroll instead, so the
  // header and the first rows stay reachable.
  const open = query !== null && items.length > 0
  useLayoutEffect(() => {
    const menu = menuRef.current
    if (!open || !menu) return
    let top = 0
    for (let element = menu.parentElement; element; element = element.parentElement) {
      const style = getComputedStyle(element)
      if (style.overflowY !== 'visible' || style.overflowX !== 'visible') {
        top = Math.max(0, element.getBoundingClientRect().top)
        break
      }
    }
    const available = Math.max(120, Math.floor(menu.getBoundingClientRect().bottom - top - 8))
    setMaxHeight((previous) => (previous === available ? previous : available))
  })

  // ↑↓ keep the selected row in view (scrolling only the list, never the panes around it).
  useLayoutEffect(() => {
    const list = itemsRef.current
    if (!open || !list || scrolledFor.current === bridge.scrollRequest) return
    scrolledFor.current = bridge.scrollRequest
    if (list.scrollHeight <= list.clientHeight) return
    if (bridge.selected === 0) {
      list.scrollTop = 0
      return
    }
    const row = list.querySelector<HTMLElement>('[aria-selected="true"]')
    if (!row) return
    const rowBox = row.getBoundingClientRect()
    const listBox = list.getBoundingClientRect()
    if (rowBox.top < listBox.top) list.scrollTop -= listBox.top - rowBox.top
    else if (rowBox.bottom > listBox.bottom) list.scrollTop += rowBox.bottom - listBox.bottom
  })

  if (!open) return null
  const selected = Math.min(bridge.selected, items.length - 1)
  return (
    <div ref={menuRef} className={styles.menu} style={maxHeight !== null ? { maxHeight } : undefined} role="listbox" aria-label="Mention suggestions">
      <div ref={itemsRef} className={styles.items}>
        {items.map((item, index) => {
          const section = item.kind === 'file' ? 'Files' : 'Apps'
          const previous = items[index - 1]
          const header = index === 0 || (previous && (previous.kind === 'file' ? 'Files' : 'Apps') !== section)
          return (
            <div key={item.id} className={styles.entry}>
              {header ? (
                <div className={styles.section} style={{ paddingTop: index === 0 ? 8 : 10 }}>
                  {section}
                </div>
              ) : null}
              <MentionRow item={item} index={index} selected={index === selected} bridge={bridge} />
            </div>
          )
        })}
      </div>
      <div className={styles.hint}>↑↓ choose · ⏎ insert · esc hide</div>
    </div>
  )
}

interface MentionRowProps {
  item: MentionItem
  index: number
  selected: boolean
  bridge: MentionBridge
}

const MentionRow = memo(function MentionRow({ item, index, selected, bridge }: MentionRowProps) {
  const { directory, name } = splitPath(item.name)
  return (
    <div
      role="option"
      aria-selected={selected}
      className={[styles.row, selected ? styles.selected : ''].filter(Boolean).join(' ')}
      // Keep the caret (and focus) in the composer while clicking.
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => bridge.accept(item.insertion)}
      onMouseEnter={() => bridge.setSelected(index)}
    >
      {item.kind === 'file' ? (
        <span className={styles.fileIcon} aria-hidden="true">
          <Icon name="file" size={12} strokeWidth={2} />
        </span>
      ) : (
        // `AppIcon(app:size:)`, or the MCP symbol for a server the catalog does not list.
        item.app ? <AppIcon app={item.app} size={24} /> : <AppIcon symbol="mcp" size={24} />
      )}
      {item.kind === 'file' ? (
        <span className={styles.fileText}>
          <span className={styles.rowTitle}>{name}</span>
          {directory.length > 0 ? (
            <span className={styles.directory}>
              <bdi dir="ltr">{directory}</bdi>
            </span>
          ) : null}
        </span>
      ) : (
        <span className={styles.appText}>
          <span className={styles.rowTitle}>@{item.name}</span>
          {item.title !== item.name ? <span className={styles.appTitle}>{item.title}</span> : null}
        </span>
      )}
      <span className={styles.spacer} />
      <span className={styles.state} style={{ color: item.tint }}>
        {item.state}
      </span>
    </div>
  )
})
