import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { closeSearchPanel, findNext, findPrevious, getSearchQuery, replaceAll, replaceNext, SearchQuery, setSearchQuery } from '@codemirror/search'
import { EditorSelection, type EditorState } from '@codemirror/state'
import { runScopeHandlers, type EditorView, type Panel, type ViewUpdate } from '@codemirror/view'
import { Button } from '../../components/Button'
import { IconButton } from '../../components/IconButton'
import { Icon } from '../../theme/icons'
import styles from './EditorFindBar.module.css'

// The find bar of the Editor's text view (NSTextView `usesFindBar` with
// `isIncrementalSearchingEnabled`, CodeEditor.swift) on CodeMirror's search
// panel: one row — the field (incremental: the selection jumps to the first
// match as you type), "2 of 7", previous / next, a Replace toggle, Done — and
// the replace row under it while Replace is on. Drawn like the Chat find
// bar so the app has one find bar; CodeMirror's own panel (lower-case
// buttons, the replace fields always shown) is not used.

/** Matches counted for "n of m" at most (a huge file is not scanned past it). */
const COUNT_LIMIT = 1000

interface FindSnapshot {
  query: SearchQuery
  total: number
  /** 1-based index of the match the selection is on, 0 when it is on none. */
  current: number
  capped: boolean
}

function snapshotOf(state: EditorState): FindSnapshot {
  const query = getSearchQuery(state)
  if (!query.valid || query.search.length === 0) return { query, total: 0, current: 0, capped: false }
  const { from, to } = state.selection.main
  let total = 0
  let current = 0
  const cursor = query.getCursor(state)
  for (let next = cursor.next(); !next.done; next = cursor.next()) {
    total += 1
    if (next.value.from === from && next.value.to === to) current = total
    if (total >= COUNT_LIMIT) return { query, total, current, capped: true }
  }
  return { query, total, current, capped: false }
}

type QueryFields = Partial<Pick<SearchQuery, 'search' | 'replace' | 'caseSensitive' | 'regexp' | 'wholeWord'>>

/** Where an incremental search starts from, and the query edits. */
class FindController {
  private readonly view: EditorView
  /** The selection's start when the search began (or after the last explicit move). */
  private start: number

  constructor(view: EditorView) {
    this.view = view
    this.start = view.state.selection.main.from
  }

  /** A move that is not the incremental jump itself (a click, next / previous) restarts from there. */
  selectionMoved(update: ViewUpdate): void {
    if (!update.selectionSet) return
    if (update.transactions.some((tr) => tr.isUserEvent('select.search.incremental'))) return
    this.start = update.state.selection.main.from
  }

  /** Replaces the panel's query; with `jump`, selects the first match at or after the start (wrapping). */
  setQuery(fields: QueryFields, jump: boolean): void {
    const state = this.view.state
    const current = getSearchQuery(state)
    const query = new SearchQuery({
      search: current.search,
      replace: current.replace,
      caseSensitive: current.caseSensitive,
      regexp: current.regexp,
      wholeWord: current.wholeWord,
      literal: current.literal,
      ...fields,
    })
    const match = jump && query.valid && query.search.length > 0 ? firstMatch(query, state, this.start) : null
    this.view.dispatch({
      effects: setSearchQuery.of(query),
      ...(match ? { selection: EditorSelection.single(match.from, match.to), scrollIntoView: true, userEvent: 'select.search.incremental' } : {}),
    })
  }
}

function firstMatch(query: SearchQuery, state: EditorState, start: number): { from: number; to: number } | null {
  let next = query.getCursor(state, start).next()
  if (next.done) next = query.getCursor(state, 0, start).next()
  return next.done ? null : next.value
}

/**
 * The `createPanel` of `search({ createPanel })`: a React root in the
 * panel's element. The first render is synchronous, so `openSearchPanel`
 * finds the `main-field` input to focus and select.
 */
export function createFindPanel(view: EditorView): Panel {
  const dom = document.createElement('div')
  dom.className = styles.panel ?? ''
  const root = createRoot(dom)
  const controller = new FindController(view)
  const render = (sync: boolean) => {
    const element = <EditorFindBar view={view} snapshot={snapshotOf(view.state)} controller={controller} />
    if (sync) flushSync(() => root.render(element))
    else root.render(element)
  }
  render(true)
  return {
    dom,
    top: true,
    // The first ⌘F / Ctrl+F: the field takes the focus with its text selected
    // (later ones are handled by `openSearchPanel` through `main-field`).
    mount() {
      const input = dom.querySelector<HTMLInputElement>('[main-field]')
      input?.focus()
      input?.select()
    },
    update(update) {
      if (!update.docChanged && !update.selectionSet && getSearchQuery(update.state) === getSearchQuery(update.startState)) return
      controller.selectionMoved(update)
      render(false)
    },
    destroy() {
      // Not inside CodeMirror's update: React may be rendering the editor.
      queueMicrotask(() => root.unmount())
    },
  }
}

interface EditorFindBarProps {
  view: EditorView
  snapshot: FindSnapshot
  controller: FindController
}

function EditorFindBar({ view, snapshot, controller }: EditorFindBarProps) {
  const { query, total, current, capped } = snapshot
  // The fields keep their own text (CodeMirror's state arrives a render
  // later); a query changed from outside (⌘F on a new selection) replaces it.
  const [search, setSearch] = useState(query.search)
  const [replace, setReplace] = useState(query.replace)
  const [replacing, setReplacing] = useState(query.replace.length > 0)
  const shown = useRef({ search: query.search, replace: query.replace })
  const replaceRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (query.search !== shown.current.search) {
      shown.current.search = query.search
      setSearch(query.search)
    }
    if (query.replace !== shown.current.replace) {
      shown.current.replace = query.replace
      setReplace(query.replace)
    }
  }, [query.search, query.replace])

  const readOnly = view.state.readOnly
  const hasMatches = total > 0
  const countLabel =
    search.length === 0 ? '' : !query.valid ? 'Invalid pattern' : total === 0 ? 'No matches' : current > 0 ? `${current} of ${total}${capped ? '+' : ''}` : `${total}${capped ? '+' : ''} matches`

  const close = () => {
    closeSearchPanel(view)
    view.focus()
  }

  /** Esc, ⌘G / F3 and the other panel keys go to CodeMirror's search keymap. */
  const panelKeys = (event: KeyboardEvent<HTMLInputElement>): boolean => {
    if (event.key === 'Escape') {
      event.preventDefault()
      close()
      return true
    }
    if (runScopeHandlers(view, event.nativeEvent, 'search-panel')) {
      event.preventDefault()
      return true
    }
    return false
  }

  const onSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return
    if (event.key === 'Enter') {
      event.preventDefault()
      if (event.shiftKey) findPrevious(view)
      else findNext(view)
      return
    }
    panelKeys(event)
  }

  const onReplaceKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return
    if (event.key === 'Enter') {
      event.preventDefault()
      replaceNext(view)
      return
    }
    panelKeys(event)
  }

  return (
    <div className={styles.bar} role="search" aria-label="Find in file">
      <div className={styles.row}>
        <FindField>
          <Icon name="search" size={12} strokeWidth={2} className={styles.fieldIcon} />
          <input
            className={styles.input}
            type="text"
            value={search}
            placeholder="Find"
            aria-label="Find"
            main-field="true"
            spellCheck={false}
            autoComplete="off"
            onChange={(event) => {
              const value = event.target.value
              shown.current.search = value
              setSearch(value)
              controller.setQuery({ search: value }, true)
            }}
            onKeyDown={onSearchKeyDown}
          />
          <OptionToggle label="Match Case" active={query.caseSensitive} onToggle={() => controller.setQuery({ caseSensitive: !query.caseSensitive }, true)}>
            Aa
          </OptionToggle>
          <OptionToggle label="Whole Words" active={query.wholeWord} onToggle={() => controller.setQuery({ wholeWord: !query.wholeWord }, true)}>
            <span className={styles.wholeWord}>ab</span>
          </OptionToggle>
          <OptionToggle label="Regular Expression" active={query.regexp} onToggle={() => controller.setQuery({ regexp: !query.regexp }, true)}>
            .*
          </OptionToggle>
        </FindField>
        <span className={styles.count} aria-live="polite">
          {countLabel}
        </span>
        <IconButton icon="chevronUp" label="Previous match" size={12} disabled={!hasMatches} onClick={() => findPrevious(view)} />
        <IconButton icon="chevronDown" label="Next match" size={12} disabled={!hasMatches} onClick={() => findNext(view)} />
        {readOnly ? null : (
          <label className={styles.replaceToggle}>
            <input
              type="checkbox"
              checked={replacing}
              onChange={(event) => {
                setReplacing(event.target.checked)
                if (event.target.checked) requestAnimationFrame(() => replaceRef.current?.focus())
              }}
            />
            Replace
          </label>
        )}
        <span className={styles.spacer} />
        <Button size="small" onClick={close}>
          Done
        </Button>
      </div>
      {replacing && !readOnly ? (
        <div className={styles.row}>
          <FindField>
            <Icon name="edit" size={12} strokeWidth={2} className={styles.fieldIcon} />
            <input
              ref={replaceRef}
              className={styles.input}
              type="text"
              value={replace}
              placeholder="Replace"
              aria-label="Replace"
              spellCheck={false}
              autoComplete="off"
              onChange={(event) => {
                const value = event.target.value
                shown.current.replace = value
                setReplace(value)
                controller.setQuery({ replace: value }, false)
              }}
              onKeyDown={onReplaceKeyDown}
            />
          </FindField>
          <Button size="small" disabled={!hasMatches} onClick={() => replaceNext(view)}>
            Replace
          </Button>
          <Button size="small" disabled={!hasMatches} onClick={() => replaceAll(view)}>
            All
          </Button>
        </div>
      ) : null}
    </div>
  )
}

/** The rounded field: border-strong while focused (no focus ring), like the Chat find bar's. */
function FindField({ children }: { children: ReactNode }) {
  const [focused, setFocused] = useState(false)
  return (
    <div
      className={[styles.field, focused ? styles.focused : ''].filter(Boolean).join(' ')}
      onFocus={() => setFocused(true)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocused(false)
      }}
    >
      {children}
    </div>
  )
}

/** A search option inside the field (the options of NSTextFinder's magnifier menu). */
function OptionToggle({ label, active, onToggle, children }: { label: string; active: boolean; onToggle: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      className={[styles.option, active ? styles.optionActive : ''].filter(Boolean).join(' ')}
      aria-label={label}
      aria-pressed={active}
      title={label}
      onMouseDown={(event) => event.preventDefault()}
      onClick={onToggle}
    >
      {children}
    </button>
  )
}
