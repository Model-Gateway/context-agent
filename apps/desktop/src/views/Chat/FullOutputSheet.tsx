// Port of `FullOutputSheet` and `ReadOnlyTextView` in
// apps/macos/ContextAgent/Views/Chat/ToolOutputView.swift: "Open full
// output" shows the whole text, read-only and selectable, with Copy all,
// Save… and Close, and the text view's find bar (mod+F).

import { invoke } from '@tauri-apps/api/core'
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type RefObject } from 'react'
import { errorText } from '../../backend/errors'
import { Button } from '../../components/Button'
import { CopyButton } from '../../components/CopyButton'
import { IconButton } from '../../components/IconButton'
import { isModKey } from '../../components/platform'
import { Sheet } from '../../components/Sheet'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { toasts } from '../../components/toasts'
import { utf8Length } from '../../models/transcript'
import { Icon } from '../../theme/icons'
import styles from './FullOutputSheet.module.css'

export interface FullOutputSheetProps {
  open: boolean
  /** Close, Esc. */
  onClose: () => void
  /** "Reading src/app.rs · Output". */
  title: string
  /** The whole text (no caps). */
  text: string
  /** The Save… panel's proposed file name ("execute-command-output.txt"). */
  suggestedFileName: string
}

/**
 * `FullOutputSheet` of the macOS app: a 760 × 540 sheet with the title, the
 * size ("1,204 lines · 48.2 KB") and Copy all on top, the whole text in a
 * read-only monospace view on `codeBackground`, and Save… / Close below.
 */
export function FullOutputSheet({ open, onClose, title, text, suggestedFileName }: FullOutputSheetProps) {
  const sizeLabel = useMemo(() => {
    const lineCount = text.split('\n').length
    const bytes = utf8Length(text)
    const size = bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`
    return `${lineCount.toLocaleString()} line${lineCount === 1 ? '' : 's'} · ${size}`
  }, [text])
  // The text view takes the focus, as the sheet's first responder is the
  // NSTextView (arrow keys and Page Down scroll it at once).
  const textRef = useRef<HTMLDivElement>(null)
  const preRef = useRef<HTMLPreElement>(null)
  const find = useTextFinder(text, preRef, textRef)

  // The NSTextView's find bar (`usesFindBar`): mod+F shows it, mod+G /
  // mod+Shift+G step through the matches. The window's commands wait while
  // the sheet is up, so these keys are the sheet's own.
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.nativeEvent.isComposing || !isModKey(event) || event.altKey) return
    const key = event.key.toLowerCase()
    if (key === 'f' && !event.shiftKey) {
      event.preventDefault()
      find.show()
    } else if (key === 'g' && find.query.length > 0) {
      event.preventDefault()
      if (event.shiftKey) find.previous()
      else find.next()
    }
  }

  return (
    <Sheet
      open={open}
      onClose={onClose}
      width={760}
      dismissOnBackdrop={false}
      initialFocus={textRef}
      aria-label={title}
      className={styles.sheet}
      style={{ padding: 0, height: 540 }}
    >
      <div className={styles.layout} onKeyDown={onKeyDown}>
        <div className={styles.header}>
          <MiddleTruncate text={title} className={styles.title} />
          <span className={styles.spacerFixed} />
          <span className={styles.size}>{sizeLabel}</span>
          <CopyButton text={text} label="Copy all" />
        </div>
        {find.visible ? <OutputFindBar find={find} /> : null}
        <div ref={textRef} tabIndex={-1} className={styles.textScroller}>
          <pre ref={preRef} className={`${styles.text} selectable`}>
            {text}
          </pre>
        </div>
        <div className={styles.footer}>
          <span className={styles.spacer} />
          <Button variant="bordered" onClick={() => void saveText(text, suggestedFileName)}>
            Save…
          </Button>
          <Button variant="prominent" onClick={onClose}>
            Close
          </Button>
        </div>
      </div>
    </Sheet>
  )
}

// MARK: - Find

/** At most this many matches are counted and highlighted. */
const MAX_MATCHES = 10_000
const MATCH_HIGHLIGHT = 'ctx-output-match'
const CURRENT_HIGHLIGHT = 'ctx-output-current'

interface TextFinder {
  visible: boolean
  query: string
  setQuery: (query: string) => void
  matchCount: number
  /** The current match (0-based; meaningful while `matchCount > 0`). */
  cursor: number
  show: () => void
  hide: () => void
  next: () => void
  previous: () => void
  /** The field, focused by `show`. */
  inputRef: RefObject<HTMLInputElement | null>
}

/** The start offsets of `query` in `text`, ignoring case (NSTextFinder's default "Contains"). */
function matchOffsets(text: string, query: string): number[] {
  if (query.length === 0) return []
  const pattern = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'giu')
  const offsets: number[] = []
  for (let match = pattern.exec(text); match !== null && offsets.length < MAX_MATCHES; match = pattern.exec(text)) {
    offsets.push(match.index)
    if (match[0].length === 0) pattern.lastIndex += 1
  }
  return offsets
}

/**
 * The find state of the read-only text: the matches of the query, the
 * current one, and their highlights (the CSS Custom Highlight API, so the
 * text stays one text node however many matches there are; where it is
 * missing the current match is selected instead). The current match is
 * scrolled into view.
 */
function useTextFinder(text: string, preRef: RefObject<HTMLPreElement | null>, scrollerRef: RefObject<HTMLDivElement | null>): TextFinder {
  const [visible, setVisible] = useState(false)
  const [query, setQueryState] = useState('')
  const [cursor, setCursor] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const offsets = useMemo(() => (visible ? matchOffsets(text, query) : []), [visible, text, query])
  const length = query.length
  const matchCount = offsets.length

  const setQuery = useCallback((value: string) => {
    setQueryState(value)
    setCursor(0)
  }, [])

  const show = useCallback(() => {
    setVisible(true)
    // Focus once the bar is in the sheet, and select what is there (mod+F again).
    window.setTimeout(() => {
      inputRef.current?.focus()
      inputRef.current?.select()
    }, 0)
  }, [])

  const hide = useCallback(() => {
    setVisible(false)
    scrollerRef.current?.focus({ preventScroll: true })
  }, [scrollerRef])

  const next = useCallback(() => {
    if (matchCount > 0) setCursor((value) => (value + 1) % matchCount)
  }, [matchCount])

  const previous = useCallback(() => {
    if (matchCount > 0) setCursor((value) => (value - 1 + matchCount) % matchCount)
  }, [matchCount])

  useEffect(() => {
    const node = preRef.current?.firstChild
    if (!visible || matchCount === 0 || !(node instanceof Text)) return
    const range = (start: number) => {
      const r = document.createRange()
      r.setStart(node, start)
      r.setEnd(node, Math.min(start + length, node.length))
      return r
    }
    const current = range(offsets[Math.min(cursor, matchCount - 1)] ?? 0)
    const registry = typeof CSS !== 'undefined' && 'highlights' in CSS ? CSS.highlights : null
    if (registry && typeof Highlight !== 'undefined') {
      registry.set(MATCH_HIGHLIGHT, new Highlight(...offsets.map(range)))
      registry.set(CURRENT_HIGHLIGHT, new Highlight(current))
    } else {
      const selection = window.getSelection()
      selection?.removeAllRanges()
      selection?.addRange(current)
    }
    // Scroll the current match into the middle third when it is out of view.
    const scroller = scrollerRef.current
    if (scroller) {
      const box = current.getBoundingClientRect()
      const frame = scroller.getBoundingClientRect()
      if (box.top < frame.top || box.bottom > frame.bottom) {
        scroller.scrollTop += box.top - frame.top - frame.height / 3
      }
    }
    return () => {
      registry?.delete(MATCH_HIGHLIGHT)
      registry?.delete(CURRENT_HIGHLIGHT)
    }
  }, [visible, offsets, cursor, matchCount, length, preRef, scrollerRef])

  // Esc in the field closes the bar, not the sheet: the sheet's Esc listener
  // is on the document's capture phase, so this one listens before it.
  useEffect(() => {
    if (!visible) return
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape' || event.isComposing || event.target !== inputRef.current) return
      event.preventDefault()
      event.stopPropagation()
      hide()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [visible, hide])

  return { visible, query, setQuery, matchCount, cursor, show, hide, next, previous, inputRef }
}

/** The find bar over the text: the field ("2 of 7" / "No matches"), previous / next and Done. */
function OutputFindBar({ find }: { find: TextFinder }) {
  const [focused, setFocused] = useState(false)
  // Capture phase: the sheet's focus trap stops Enter from bubbling.
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing || event.key !== 'Enter') return
    event.preventDefault()
    if (event.shiftKey) find.previous()
    else find.next()
  }
  const countLabel = find.query.length === 0 ? '' : find.matchCount === 0 ? 'No matches' : `${find.cursor + 1} of ${find.matchCount.toLocaleString()}${find.matchCount >= MAX_MATCHES ? '+' : ''}`
  return (
    <div className={styles.findBar}>
      <div className={[styles.findField, focused ? styles.findFocused : ''].filter(Boolean).join(' ')}>
        <Icon name="search" size={13} strokeWidth={2} className={styles.findIcon} />
        <input
          ref={find.inputRef}
          className={styles.findInput}
          type="text"
          value={find.query}
          placeholder="Find"
          aria-label="Find in output"
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => find.setQuery(event.target.value)}
          onKeyDownCapture={onKeyDown}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
        />
        <span className={styles.findCount} aria-live="polite">
          {countLabel}
        </span>
      </div>
      <IconButton icon="chevronUp" label="Previous match" size={12} disabled={find.matchCount === 0} onClick={find.previous} />
      <IconButton icon="chevronDown" label="Next match" size={12} disabled={find.matchCount === 0} onClick={find.next} />
      <span className={styles.spacer} />
      <Button size="small" onClick={find.hide}>
        Done
      </Button>
    </div>
  )
}

/// Save… (`NSSavePanel` titled "Save Output" with the suggested name):
/// the shell's save dialog (`fs_save_text`, the only command that writes
/// where the user chooses), with the file type taken from the suggested
/// name. Cancel is silent, like the Swift panel; a write that fails says so
/// in a toast (the Swift `try?` drops it).
async function saveText(text: string, suggestedName: string): Promise<void> {
  const dot = suggestedName.lastIndexOf('.')
  const extension = dot > 0 ? suggestedName.slice(dot + 1) : undefined
  try {
    await invoke<string | null>('fs_save_text', {
      title: 'Save Output',
      default_name: suggestedName,
      content: text,
      extension,
    })
  } catch (error) {
    const message = errorText(error)
    toasts.push({ message: `Could not save the output: ${message}`, level: 'error', details: message })
  }
}
