import { memo, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { startCompletion, type Completion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete'
import type { EditorState } from '@codemirror/state'
import type { EditorView } from '@codemirror/view'
import type { LspCompletionItem, LspRange, SymbolInfo } from '../../backend/types'
import { Icon, type IconName } from '../../theme/icons'
import styles from './CompletionPanel.module.css'

// Port of apps/macos/ContextAgent/Views/Editor/CompletionPanel.swift: the
// editor's "type sense". `CompletionItem` is the Swift struct;
// `CompletionSession` is `CompletionController` as a CodeMirror completion
// source (local candidates at once, the language server's or the index's
// items merged in when they arrive); `CompletionPanel` is
// `CompletionListView` / `CompletionRow`, drawn in place of CodeMirror's own
// tooltip (which CodeEditor hides) so the popup looks like the Swift panel.

// MARK: - CompletionItem

/** `CompletionItem.Source`. */
export type CompletionItemSource =
  /** The language server's item (`lsp/completion`). */
  | { kind: 'server'; symbolKind: string }
  /** A definition the index knows (`symbols/find`), or one found in the buffer itself (unsaved yet). */
  | { kind: 'symbol'; symbolKind: string; path: string; line: number }
  | { kind: 'keyword' }
  /** A word already in the buffer. */
  | { kind: 'word' }

/** `CompletionItem` (CompletionPanel.swift): one row of the completion popup. */
export interface CompletionItem {
  readonly id: string
  readonly label: string
  readonly detail: string | null
  readonly source: CompletionItemSource
  /** What goes into the text; the label unless the server said otherwise. */
  readonly insertText: string
  /** The server's replace range (0-based) when it gave one. */
  readonly replaceRange: LspRange | null
}

/** `CompletionItem(server:)`. */
export function serverCompletionItem(item: LspCompletionItem): CompletionItem {
  return {
    id: `server:${item.kind}:${item.label}:${item.insert_text}`,
    label: item.label,
    detail: item.detail ?? (item.kind === 'text' ? null : item.kind.replace(/_/g, ' ')),
    source: { kind: 'server', symbolKind: item.kind },
    insertText: item.insert_text,
    replaceRange: item.range ?? null,
  }
}

/** `CompletionItem(symbol:)`. */
export function symbolCompletionItem(symbol: SymbolInfo): CompletionItem {
  return {
    id: `symbol:${symbol.path}:${symbol.line}:${symbol.name}`,
    label: symbol.name,
    detail: symbol.signature ?? (symbol.parent != null ? `${symbol.kind} in ${symbol.parent}` : symbol.kind),
    source: { kind: 'symbol', symbolKind: symbol.kind, path: symbol.path, line: symbol.line },
    insertText: symbol.name,
    replaceRange: null,
  }
}

/** `CompletionItem(bufferDefinition:kind:signature:path:line:)`: a definition spotted in the buffer (`fn name`, `class Name`, …). */
export function bufferDefinitionItem(name: string, kind: string, signature: string, path: string, line: number): CompletionItem {
  return {
    id: `symbol:${path}:${line}:${name}`,
    label: name,
    detail: signature,
    source: { kind: 'symbol', symbolKind: kind, path, line },
    insertText: name,
    replaceRange: null,
  }
}

/** `CompletionItem(keyword:)`. */
export function keywordCompletionItem(keyword: string): CompletionItem {
  return { id: `keyword:${keyword}`, label: keyword, detail: 'keyword', source: { kind: 'keyword' }, insertText: keyword, replaceRange: null }
}

/** `CompletionItem(word:)`. */
export function wordCompletionItem(word: string): CompletionItem {
  return { id: `word:${word}`, label: word, detail: null, source: { kind: 'word' }, insertText: word, replaceRange: null }
}

/** `CompletionItem.path`: the file of an index or buffer definition. */
export function completionItemPath(item: CompletionItem): string | null {
  return item.source.kind === 'symbol' ? item.source.path : null
}

/** `CompletionItem.systemImage`. */
export function completionItemIcon(item: CompletionItem): IconName {
  switch (item.source.kind) {
    case 'server':
    case 'symbol':
      return symbolKindIcon(item.source.symbolKind)
    case 'keyword':
      return 'symbolKeyword'
    case 'word':
      return 'symbolWord'
  }
}

/** `CompletionItem.systemImage(forSymbolKind:)`: also the status bar's symbol glyph. */
export function symbolKindIcon(kind: string): IconName {
  switch (kind) {
    case 'function':
    case 'method':
    case 'macro':
    case 'constructor':
      return 'symbolFunction'
    case 'struct':
    case 'class':
    case 'interface':
    case 'trait':
    case 'impl':
    case 'component':
    case 'type_parameter':
      return 'shippingBox'
    case 'enum':
    case 'enum_member':
      return 'symbolEnum'
    case 'type_alias':
      return 'symbolTypeAlias'
    case 'variable':
    case 'field':
    case 'constant':
    case 'property':
    case 'value':
      return 'symbolVariable'
    case 'module':
    case 'namespace':
    case 'section':
    case 'file':
    case 'folder':
      return 'folder'
    case 'keyword':
      return 'symbolKeyword'
    case 'snippet':
      return 'symbolSnippet'
    case 'text':
      return 'symbolWord'
    default:
      return 'symbolOther'
  }
}

// MARK: - CompletionSession (`CompletionController`)

/** What the session asks of the editor (`CodeEditor.Coordinator`'s side of the popup). */
export interface CompletionHost {
  /** The view, for refreshing the popup when remote items arrive. */
  view(): EditorView | null
  /** A language server is attached to the document. */
  hasServer(): boolean
  /** The server's completion trigger characters. */
  triggers(): readonly string[]
  /** `isWord(_:)` for one typed character. */
  isWordCharacter(character: string): boolean
  /** The single character the last edit typed, else null. */
  lastTyped(state: EditorState): string | null
  /** `identifierRange(endingAt:)`'s start, or `pos` when no identifier ends there. */
  wordStart(state: EditorState, pos: number): number
  /** `localCandidates(prefix:hasServer:)`: buffer definitions, keywords, buffer words. */
  localCandidates(prefix: string, hasServer: boolean): CompletionItem[]
  /** The `remote` closure: `lsp/completion` with a server, else `symbols/find` prefix. */
  remote(prefix: string, pos: number, hasServer: boolean): Promise<CompletionItem[]>
  /** `insert(_:replacing:)`. */
  insert(view: EditorView, item: CompletionItem, from: number, to: number, completion: Completion): void
}

/** The popup's row cap (`show(resetSelection:)`). */
const MAX_ITEMS = 80
/** The pause before the remote request (`present`'s 60 ms sleep). */
const REMOTE_DELAY_MS = 60

/**
 * `CompletionController` (CompletionPanel.swift) as a CodeMirror completion
 * source: local candidates (keywords, words of the buffer) show at once and
 * the server's / index's items are merged in when `remote` returns, those
 * first. Keeps the remote items of the previous prefix while they still fit.
 */
export class CompletionSession {
  /** `model.prefix`. */
  prefix = ''
  /** `wordRange.location`. */
  from = 0
  /** The popup is up (CodeEditor clears it when CodeMirror closes the completion). */
  private visible = false
  /** Local candidates came back empty while a remote request is in flight. */
  private awaitingRemote = false
  /** The user asked for the popup (Ctrl+Space; Esc on macOS, NSTextView's `complete:`). */
  private requested = false
  private remoteItems: CompletionItem[] = []
  private remoteTimer: ReturnType<typeof setTimeout> | null = null
  /** The remote request was sent and has not answered yet. */
  private inFlight = false
  private generation = 0
  /** The completion objects of the current prefix by item id (same object → CodeMirror keeps the selection). */
  private readonly objects = new Map<string, Completion>()
  private readonly items = new WeakMap<Completion, CompletionItem>()
  private readonly host: CompletionHost

  constructor(host: CompletionHost) {
    this.host = host
  }

  /** The next (explicit) query was asked for by the user, not a refresh with the remote items. */
  request(): void {
    this.requested = true
  }

  /** `offerCompletion()` + `present(prefix:wordRange:local:remote:)`. */
  readonly source = (context: CompletionContext): CompletionResult | null => {
    const { state, pos } = context
    const host = this.host
    const requested = this.requested && context.explicit
    this.requested = false
    const hasServer = host.hasServer()
    const from = host.wordStart(state, pos)
    const prefix = state.sliceDoc(from, pos)
    const minimum = hasServer ? 1 : 2
    const typedCharacter = host.lastTyped(state)
    const typed = typedCharacter !== null && host.isWordCharacter(typedCharacter) && prefix.length >= minimum
    const triggered = hasServer && typedCharacter !== null && host.triggers().includes(typedCharacter)
    const refining = (this.visible || this.awaitingRemote) && prefix.length >= 1
    if (!state.selection.main.empty || !(typed || triggered || refining || requested)) {
      this.dismiss()
      return null
    }
    const prefixChanged = this.prefix !== prefix
    this.prefix = prefix
    this.from = from
    const local = host.localCandidates(prefix, hasServer)
    // An explicit query is the remote items' refresh, unless the user asked.
    if (!context.explicit || requested) {
      // Items from the previous prefix still fit while the user keeps typing it.
      const lowered = prefix.toLowerCase()
      this.remoteItems = this.remoteItems.filter((item) => prefix.length === 0 || item.label.toLowerCase().startsWith(lowered))
      this.fetchRemote(prefix, pos, hasServer, state)
    }
    const items = this.merged(local, prefix)
    if (prefixChanged) this.objects.clear()
    if (items.length === 0) {
      this.visible = false
      this.awaitingRemote = this.remoteTimer !== null || this.inFlight
      return null
    }
    this.visible = true
    this.awaitingRemote = false
    return { from, to: pos, options: items.map((item) => this.completion(item)), filter: false }
  }

  /** The `CompletionItem` behind one of the source's completions. */
  itemOf(completion: Completion): CompletionItem | null {
    return this.items.get(completion) ?? null
  }

  /// CodeMirror closed the completion (Esc, the caret moved away, blur, an
  /// accepted item): `dismiss()`, unless the source itself closed it while a
  /// remote request it is waiting for is still out.
  closed(): void {
    if (this.awaitingRemote) {
      this.visible = false
      return
    }
    if (this.visible || this.remoteTimer !== null || this.inFlight) this.dismiss()
  }

  /** `dismiss()`: drops the pending remote request. */
  dismiss(): void {
    if (this.remoteTimer !== null) clearTimeout(this.remoteTimer)
    this.remoteTimer = null
    this.inFlight = false
    this.generation += 1
    this.visible = false
    this.awaitingRemote = false
  }

  /// The `remoteTask`: after 60 ms, the server's items (or the index's
  /// symbols); when they arrive for the prefix still being typed, the popup
  /// is refreshed with them merged in, the selection kept.
  private fetchRemote(prefix: string, pos: number, hasServer: boolean, state: EditorState): void {
    if (this.remoteTimer !== null) clearTimeout(this.remoteTimer)
    this.generation += 1
    const generation = this.generation
    const doc = state.doc
    this.inFlight = false
    this.remoteTimer = setTimeout(() => {
      this.remoteTimer = null
      this.inFlight = true
      void this.host.remote(prefix, pos, hasServer).then((items) => {
        if (generation !== this.generation) return
        this.inFlight = false
        this.remoteItems = items
        const view = this.host.view()
        if (!view || !view.hasFocus || view.state.doc !== doc || view.state.selection.main.head !== pos) {
          this.awaitingRemote = false
          return
        }
        if (!this.visible && items.length === 0) {
          this.awaitingRemote = false
          return
        }
        startCompletion(view)
      })
    }, REMOTE_DELAY_MS)
  }

  /// `show(resetSelection:)`'s list: remote then local, one per label and
  /// path, case-sensitive prefix matches before the case-insensitive ones
  /// (stably), at most 80.
  private merged(local: readonly CompletionItem[], prefix: string): CompletionItem[] {
    const seen = new Set<string>()
    const items: CompletionItem[] = []
    for (const item of [...this.remoteItems, ...local]) {
      const key = item.label + (completionItemPath(item) ?? '')
      if (seen.has(key)) continue
      seen.add(key)
      items.push(item)
    }
    const exact = items.filter((item) => item.label.startsWith(prefix))
    const loose = items.filter((item) => !item.label.startsWith(prefix))
    return [...exact, ...loose].slice(0, MAX_ITEMS)
  }

  private completion(item: CompletionItem): Completion {
    const existing = this.objects.get(item.id)
    if (existing) return existing
    const completion: Completion = {
      label: item.label,
      apply: (view, picked, from, to) => this.host.insert(view, item, from, to, picked),
    }
    this.objects.set(item.id, completion)
    this.items.set(completion, item)
    return completion
  }
}

// MARK: - CompletionPanel (`CompletionListView`)

/** `rowHeight`. */
const ROW_HEIGHT = 24
/** `maxRows`. */
const MAX_ROWS = 8
/** `width`. */
const PANEL_WIDTH = 440
/** The list's vertical padding (5) on each side plus the 1 px border. */
const PANEL_CHROME = 12

export interface CompletionPanelProps {
  view: EditorView
  items: readonly CompletionItem[]
  selectedIndex: number
  /** The typed prefix, drawn bold in the labels. */
  prefix: string
  /** Where the word being completed starts (the anchor). */
  from: number
  /** A click on a row: select it and accept it. */
  onPick: (index: number) => void
}

/**
 * `CompletionListView` (CompletionPanel.swift): the popup under the word —
 * 440 px wide, up to eight 24 px rows, `surface` with a `borderStrong`
 * hairline and 8 px corners, the selected row in `accentSoft`. Placed 30 px
 * left of the word's start (so the labels line up with it), 4 px below the
 * line, or above it when there is no room. Mouse presses keep the editor
 * focused; ↑ ↓ ⏎ ⇥ Esc stay with the editor's keymap.
 */
export const CompletionPanel = memo(function CompletionPanel({ view, items, selectedIndex, prefix, from, onPick }: CompletionPanelProps) {
  const listRef = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null)
  const rows = Math.min(items.length, MAX_ROWS)
  const height = rows * ROW_HEIGHT + PANEL_CHROME

  useLayoutEffect(() => {
    const place = () => {
      const anchor = view.coordsAtPos(from, 1) ?? view.coordsAtPos(view.state.selection.main.head)
      if (!anchor) {
        setPosition(null)
        return
      }
      const left = Math.max(0, Math.min(anchor.left - 30, window.innerWidth - PANEL_WIDTH))
      let top = anchor.bottom + 4
      if (top + height > window.innerHeight) top = anchor.top - height - 4
      setPosition((current) => (current && current.left === left && current.top === top ? current : { left, top }))
    }
    place()
    const scroller = view.scrollDOM
    scroller.addEventListener('scroll', place, { passive: true })
    window.addEventListener('resize', place)
    // The editor moves or resizes under the popup (a banner appears above it,
    // the inspector opens): follow the word.
    const observer = new ResizeObserver(place)
    observer.observe(view.dom)
    return () => {
      scroller.removeEventListener('scroll', place)
      window.removeEventListener('resize', place)
      observer.disconnect()
    }
  }, [view, from, height, items])

  // `proxy.scrollTo(model.items[index].id)` on selection change.
  useLayoutEffect(() => {
    const row = listRef.current?.children[selectedIndex] as HTMLElement | undefined
    row?.scrollIntoView({ block: 'nearest' })
  }, [selectedIndex, items])

  return createPortal(
    <div
      className={styles.panel}
      style={position ? { left: position.left, top: position.top, height } : { left: -9999, top: -9999, height }}
      role="listbox"
      aria-label="Completions"
      onMouseDown={(event) => event.preventDefault()}
    >
      <div ref={listRef} className={styles.list}>
        {items.map((item, index) => (
          <CompletionRow key={item.id} item={item} index={index} prefix={prefix} selected={index === selectedIndex} onPick={onPick} />
        ))}
      </div>
    </div>,
    document.body,
  )
})

interface CompletionRowProps {
  item: CompletionItem
  index: number
  prefix: string
  selected: boolean
  onPick: (index: number) => void
}

/** `CompletionRow`: glyph · label (typed prefix bold) · detail · path. */
const CompletionRow = memo(function CompletionRow({ item, index, prefix, selected, onPick }: CompletionRowProps) {
  const path = completionItemPath(item)
  const matches = item.label.toLowerCase().startsWith(prefix.toLowerCase())
  return (
    <div
      role="option"
      aria-selected={selected}
      aria-label={item.detail !== null ? `${item.label}, ${item.detail}` : item.label}
      className={[styles.row, selected ? styles.selected : ''].filter(Boolean).join(' ')}
      onClick={() => onPick(index)}
    >
      <span className={styles.icon}>
        <Icon name={completionItemIcon(item)} size={11} strokeWidth={2} />
      </span>
      <span className={styles.label}>
        {matches ? (
          <>
            <b>{item.label.slice(0, prefix.length)}</b>
            {item.label.slice(prefix.length)}
          </>
        ) : (
          item.label
        )}
      </span>
      {item.detail !== null ? <span className={styles.detail}>{item.detail}</span> : null}
      <span className={styles.spacer} aria-hidden="true" />
      {path !== null ? (
        <span className={styles.path}>
          <bdi>{path}</bdi>
        </span>
      ) : null}
    </div>
  )
})
