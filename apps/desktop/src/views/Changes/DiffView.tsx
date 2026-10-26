import { memo, useCallback, useEffect, useMemo, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from 'react'
import { Virtuoso } from 'react-virtuoso'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { Icon } from '../../theme/icons'
import styles from './DiffView.module.css'

// Port of apps/macos/ContextAgent/Views/Changes/DiffView.swift. The Swift
// view lays the whole diff into one read-only NSTextView with a gutter view
// over its leading edge; here it is a virtualised list (react-virtuoso) of
// one element per diff line: a sticky gutter (old / new line numbers and the
// +/− marker over the row's tint) followed by the line's text.

/** `DiffView.expandedHunkLimit`: hunks from this index on start collapsed. */
const EXPANDED_HUNK_LIMIT = 20
/// `DiffView.synchronousParseLimit`: diffs up to this size are parsed while
/// rendering (no blank frame); larger ones after a "Rendering diff…" frame.
const SYNCHRONOUS_PARSE_LIMIT = 64 * 1024

// MARK: - Parsing

/** `ParsedDiff.LineKind`. */
export type DiffLineKind = 'context' | 'addition' | 'deletion' | 'noNewline'

/** `ParsedDiff.Line`. */
export interface DiffLine {
  id: number
  kind: DiffLineKind
  oldNumber: number | null
  newNumber: number | null
  /** Tabs expanded; empty lines carry a single space so rows keep their height. */
  displayText: string
}

/** `ParsedDiff.Line.marker`. */
export function diffLineMarker(kind: DiffLineKind): string {
  switch (kind) {
    case 'context':
      return ' '
    case 'addition':
      return '+'
    case 'deletion':
      return '−'
    case 'noNewline':
      return '\\'
  }
}

/** `ParsedDiff.Hunk`. */
export interface DiffHunk {
  /** Global hunk index across files (collapse threshold). */
  id: number
  header: string
  lines: DiffLine[]
}

/** `ParsedDiff.File`. */
export interface DiffFile {
  id: number
  oldPath: string | null
  newPath: string | null
  isNew: boolean
  isDeleted: boolean
  isBinary: boolean
  renamedFrom: string | null
  hunks: DiffHunk[]
  additions: number
  deletions: number
}

/** `ParsedDiff.File.displayPath`. */
export function diffFileDisplayPath(file: DiffFile): string {
  return file.newPath ?? file.oldPath ?? 'file'
}

/** `ParsedDiff`: a parsed unified diff. Plain data; built once per diff text. */
export interface ParsedDiff {
  files: DiffFile[]
  additions: number
  deletions: number
  hunkCount: number
  maxLineNumber: number
  /** Longest line in UTF-16 code units (a cheap proxy for rendered width). */
  maxLineLength: number
}

function makeFile(id: number): DiffFile {
  return { id, oldPath: null, newPath: null, isNew: false, isDeleted: false, isBinary: false, renamedFrom: null, hunks: [], additions: 0, deletions: 0 }
}

function stripPrefix(path: string, prefix: string): string {
  return path.startsWith(prefix) ? path.slice(prefix.length) : path
}

function display(text: string): string {
  if (text.length === 0) return ' '
  if (text.includes('\t')) return text.replace(/\t/g, '    ')
  return text
}

/** Swift's `Int(_:)` on a token: an optional sign and digits only. */
function parseInteger(token: string | undefined): number | null {
  if (token === undefined || !/^[+-]?\d+$/.test(token)) return null
  return Number.parseInt(token, 10)
}

/** `@@ -12,7 +12,8 @@` → (12, 12); missing numbers default to 1. */
function hunkStarts(line: string): { old: number; new: number } {
  let old = 1
  let next = 1
  for (const token of line.split(' ')) {
    if (token.length === 0) continue
    const first = token
      .slice(1)
      .split(',')
      .find((part) => part.length > 0)
    if (token.startsWith('-') && old === 1) {
      const value = parseInteger(first)
      if (value !== null) old = value
    } else if (token.startsWith('+') && next === 1) {
      const value = parseInteger(first)
      if (value !== null) next = value
    }
  }
  return { old, new: next }
}

/** `ParsedDiff.parse(_:)`. */
export function parseDiff(text: string): ParsedDiff {
  const result: ParsedDiff = { files: [], additions: 0, deletions: 0, hunkCount: 0, maxLineNumber: 0, maxLineLength: 0 }
  const parts = text.split('\n')
  if (parts[parts.length - 1] === '') parts.pop()

  let current: DiffFile | null = null
  let hunk: DiffHunk | null = null
  let oldLine = 0
  let newLine = 0
  let nextLineId = 0

  const closeHunk = () => {
    if (hunk) {
      current?.hunks.push(hunk)
      hunk = null
    }
  }
  const closeFile = () => {
    closeHunk()
    if (current) {
      result.files.push(current)
      current = null
    }
  }

  for (const raw of parts) {
    const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw

    if (line.startsWith('diff --git ')) {
      closeFile()
      const file = makeFile(result.files.length)
      const rest = line.slice('diff --git '.length)
      const split = rest.indexOf(' b/')
      if (split >= 0) {
        file.oldPath = stripPrefix(rest.slice(0, split), 'a/')
        file.newPath = rest.slice(split + 3)
      } else {
        file.newPath = rest
      }
      current = file
      continue
    }

    if (line.startsWith('@@')) {
      closeHunk()
      if (!current) current = makeFile(result.files.length)
      const starts = hunkStarts(line)
      oldLine = starts.old
      newLine = starts.new
      hunk = { id: result.hunkCount, header: line, lines: [] }
      result.hunkCount += 1
      continue
    }

    const open: DiffHunk | null = hunk
    if (!open) {
      // File header area (or a diff without `diff --git`).
      if (!current) {
        if (line.length === 0) continue
        current = makeFile(result.files.length)
      }
      const file: DiffFile = current
      if (line.startsWith('--- ')) {
        const path = line.slice(4)
        if (path === '/dev/null') file.isNew = true
        else file.oldPath = stripPrefix(path, 'a/')
      } else if (line.startsWith('+++ ')) {
        const path = line.slice(4)
        if (path === '/dev/null') file.isDeleted = true
        else file.newPath = stripPrefix(path, 'b/')
      } else if (line.startsWith('new file mode')) {
        file.isNew = true
      } else if (line.startsWith('deleted file mode')) {
        file.isDeleted = true
      } else if (line.startsWith('rename from ')) {
        file.renamedFrom = line.slice('rename from '.length)
      } else if (line.startsWith('rename to ')) {
        file.newPath = line.slice('rename to '.length)
      } else if (line.startsWith('Binary files') || line.startsWith('GIT binary patch')) {
        file.isBinary = true
      }
      continue
    }

    const file = current as DiffFile | null
    let entry: DiffLine
    if (line.startsWith('\\')) {
      entry = { id: nextLineId, kind: 'noNewline', oldNumber: null, newNumber: null, displayText: display(line) }
    } else if (line.startsWith('+')) {
      entry = { id: nextLineId, kind: 'addition', oldNumber: null, newNumber: newLine, displayText: display(line.slice(1)) }
      newLine += 1
      result.additions += 1
      if (file) file.additions += 1
    } else if (line.startsWith('-')) {
      entry = { id: nextLineId, kind: 'deletion', oldNumber: oldLine, newNumber: null, displayText: display(line.slice(1)) }
      oldLine += 1
      result.deletions += 1
      if (file) file.deletions += 1
    } else {
      const body = line.startsWith(' ') ? line.slice(1) : line
      entry = { id: nextLineId, kind: 'context', oldNumber: oldLine, newNumber: newLine, displayText: display(body) }
      oldLine += 1
      newLine += 1
    }
    nextLineId += 1
    result.maxLineNumber = Math.max(result.maxLineNumber, oldLine, newLine)
    result.maxLineLength = Math.max(result.maxLineLength, entry.displayText.length)
    open.lines.push(entry)
  }
  closeFile()
  return result
}

// MARK: - Rows

/** `DiffRow`: one paragraph of the diff and how to draw its gutter and background. */
type DiffRow =
  | { kind: 'fileHeader'; key: string; file: DiffFile; first: boolean }
  | { kind: 'note'; key: string; message: string }
  | { kind: 'hunkHeader'; key: string; id: number; header: string; expanded: boolean; lineCount: number }
  | { kind: 'line'; key: string; line: DiffLine }

/** `DiffDocument.build(_:showFileHeaders:isExpanded:)`. */
function buildRows(model: ParsedDiff, showFileHeaders: boolean, isExpanded: (id: number) => boolean): DiffRow[] {
  const rows: DiffRow[] = []
  for (const [fileIndex, file] of model.files.entries()) {
    if (showFileHeaders) rows.push({ kind: 'fileHeader', key: `f${file.id}`, file, first: fileIndex === 0 })
    if (file.hunks.length === 0) {
      let message: string
      if (file.isBinary) message = 'Binary file changed; no text diff.'
      else if (file.renamedFrom !== null) message = 'Renamed without content changes.'
      else message = 'No text changes.'
      rows.push({ kind: 'note', key: `n${file.id}`, message })
    }
    for (const hunk of file.hunks) {
      const expanded = isExpanded(hunk.id)
      rows.push({ kind: 'hunkHeader', key: `h${hunk.id}`, id: hunk.id, header: hunk.header, expanded, lineCount: hunk.lines.length })
      if (!expanded) continue
      for (const line of hunk.lines) rows.push({ kind: 'line', key: `l${line.id}`, line })
    }
  }
  return rows
}

// MARK: - DiffView

export interface DiffViewProps {
  /** The unified diff text (`git diff` output). */
  diff: string
  /** The backend capped the diff: a banner says so. */
  truncated?: boolean
  /// False when the caller already names the file (the Changes pane
  /// header), so the path is not shown twice.
  showFileHeaders?: boolean
}

/**
 * `DiffView` (Views/Changes/DiffView.swift): unified diff renderer — a file
 * header per `diff --git` block, `@@` hunk headers in `text-tertiary`,
 * additions tinted `success`, deletions `danger`, monospace old / new line
 * numbers in the gutter. Hunks beyond the first 20 start collapsed ("Show
 * hunk"; clicking a `@@` header toggles its hunk), and a banner says so
 * when the backend truncated the diff.
 */
export function DiffView({ diff, truncated = false, showFileHeaders = true }: DiffViewProps) {
  const initialModel = useMemo(() => (diff.length <= SYNCHRONOUS_PARSE_LIMIT ? parseDiff(diff) : null), [diff])
  const [deferred, setDeferred] = useState<{ source: string; model: ParsedDiff } | null>(null)
  // `toggledHunks`, reset whenever the diff text changes.
  const [toggled, setToggled] = useState<{ source: string; hunks: ReadonlySet<number> }>({ source: diff, hunks: new Set() })

  useEffect(() => {
    if (initialModel) return
    let cancelled = false
    const source = diff
    const handle = window.setTimeout(() => {
      const model = parseDiff(source)
      if (!cancelled) setDeferred({ source, model })
    }, 0)
    return () => {
      cancelled = true
      window.clearTimeout(handle)
    }
  }, [diff, initialModel])

  const model = initialModel ?? (deferred && deferred.source === diff ? deferred.model : null)
  const toggledHunks = toggled.source === diff ? toggled.hunks : EMPTY_SET

  const onToggleHunk = useCallback(
    (id: number) => {
      setToggled((previous) => {
        const hunks = new Set(previous.source === diff ? previous.hunks : [])
        if (hunks.has(id)) hunks.delete(id)
        else hunks.add(id)
        return { source: diff, hunks }
      })
    },
    [diff],
  )

  let body: ReactNode
  if (model) {
    body = model.files.length === 0 ? (
      <EmptyPlaceholder icon="changes" title="No differences" subtitle="The diff is empty." />
    ) : (
      <DiffBody model={model} showFileHeaders={showFileHeaders} toggledHunks={toggledHunks} onToggleHunk={onToggleHunk} />
    )
  } else {
    body = (
      <div className={styles.loading}>
        <BrailleSpinner size={16} />
        <span className={styles.loadingText}>Rendering diff…</span>
      </div>
    )
  }

  return (
    <div className={styles.diffView}>
      {truncated ? <TruncatedBanner /> : null}
      {body}
    </div>
  )
}

const EMPTY_SET: ReadonlySet<number> = new Set()

// MARK: - Body

interface DiffBodyProps {
  model: ParsedDiff
  showFileHeaders: boolean
  toggledHunks: ReadonlySet<number>
  onToggleHunk: (id: number) => void
}

/// `DiffBody`: the rows in a virtualised list. Each row is the gutter
/// (sticky at the leading edge, over the row's tint) and the line's text,
/// indented past it; only the visible rows are in the DOM.
function DiffBody({ model, showFileHeaders, toggledHunks, onToggleHunk }: DiffBodyProps) {
  const rows = useMemo(
    () => buildRows(model, showFileHeaders, (id) => (id < EXPANDED_HUNK_LIMIT) !== toggledHunks.has(id)),
    [model, showFileHeaders, toggledHunks],
  )
  const digits = Math.max(2, String(model.maxLineNumber).length)
  const style = { '--diff-digits': digits, '--diff-max-line': model.maxLineLength } as CSSProperties

  return (
    <div className={styles.body} style={style} role="region" aria-label="Diff">
      <Virtuoso
        className={styles.scroller}
        data={rows}
        computeItemKey={(_, row) => row.key}
        defaultItemHeight={15}
        increaseViewportBy={{ top: 400, bottom: 400 }}
        components={VIRTUOSO_COMPONENTS}
        itemContent={(_, row) => <DiffRowView row={row} onToggleHunk={onToggleHunk} />}
      />
    </div>
  )
}

/** `textContainerInset` height 6 above and below the text. */
function Inset() {
  return <div className={styles.inset} aria-hidden />
}

const VIRTUOSO_COMPONENTS = { Header: Inset, Footer: Inset }

interface DiffRowViewProps {
  row: DiffRow
  onToggleHunk: (id: number) => void
}

/// One row: its tint across the full width (`DiffLayoutManager`), the
/// gutter's numbers and marker (`DiffGutterView`) and the text.
const DiffRowView = memo(function DiffRowView({ row, onToggleHunk }: DiffRowViewProps) {
  switch (row.kind) {
    case 'line': {
      const { line } = row
      const marker = diffLineMarker(line.kind)
      return (
        <div className={`${styles.row} ${LINE_CLASS[line.kind]}`}>
          <Gutter oldNumber={line.oldNumber} newNumber={line.newNumber} marker={marker.trim().length > 0 ? marker : null} />
          <span className={`${styles.text} selectable`}>{line.displayText}</span>
        </div>
      )
    }
    case 'hunkHeader': {
      const toggle = () => onToggleHunk(row.id)
      const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          toggle()
        }
      }
      return (
        <div
          className={`${styles.row} ${styles.hunkRow}`}
          role="button"
          tabIndex={0}
          aria-expanded={row.expanded}
          onClick={(event) => {
            // A drag that selected text is not a click on the header.
            if (window.getSelection()?.isCollapsed === false && event.detail <= 1) return
            toggle()
          }}
          onKeyDown={onKeyDown}
        >
          <Gutter oldNumber={null} newNumber={null} marker={null} />
          <span className={`${styles.text} selectable`}>
            {row.header}
            {row.expanded ? null : <span className={styles.showHunk}>{`   Show hunk · ${row.lineCount} line${row.lineCount === 1 ? '' : 's'}`}</span>}
          </span>
        </div>
      )
    }
    case 'note':
      return (
        <div className={`${styles.row} ${styles.noteRow}`}>
          <Gutter oldNumber={null} newNumber={null} marker={null} />
          <span className={`${styles.text} selectable`}>{row.message}</span>
        </div>
      )
    case 'fileHeader': {
      const { file } = row
      const notes: string[] = []
      if (file.isNew) notes.push('new file')
      if (file.isDeleted) notes.push('deleted')
      if (file.renamedFrom !== null) notes.push(`renamed from ${file.renamedFrom}`)
      if (file.isBinary) notes.push('binary')
      return (
        <div className={`${styles.row} ${styles.fileRow} ${row.first ? styles.firstFileRow : ''}`}>
          <Gutter oldNumber={null} newNumber={null} marker={null} />
          <span className={`${styles.text} selectable`}>
            {diffFileDisplayPath(file)}
            {file.additions > 0 || file.deletions > 0 ? (
              <>
                {'   '}
                <span className={styles.fileAdded}>{`+${file.additions}`}</span>
                <span className={styles.fileRemoved}>{` −${file.deletions}`}</span>
              </>
            ) : null}
            {notes.length > 0 ? <span className={styles.fileNotes}>{`   ${notes.join(' · ')}`}</span> : null}
          </span>
        </div>
      )
    }
  }
})

const LINE_CLASS: Record<DiffLineKind, string> = {
  context: styles.context ?? '',
  addition: styles.addition ?? '',
  deletion: styles.deletion ?? '',
  noNewline: styles.noNewline ?? '',
}

interface GutterProps {
  oldNumber: number | null
  newNumber: number | null
  /** The +/− (or `\`) marker; null draws none. */
  marker: string | null
}

/// `DiffGutterView`'s drawing for one row: old and new numbers right-aligned
/// in their columns, the marker centred in a 16 px column, over the row's
/// tint. Not selectable, so copying the diff copies only its text.
function Gutter({ oldNumber, newNumber, marker }: GutterProps) {
  return (
    <span className={styles.gutter} aria-hidden>
      <span className={styles.number}>{oldNumber ?? ''}</span>
      <span className={styles.number}>{newNumber ?? ''}</span>
      <span className={styles.marker}>{marker ?? ''}</span>
    </span>
  )
}

// MARK: - Banner

/** `TruncatedBanner`. */
function TruncatedBanner() {
  return (
    <div className={styles.banner} role="note">
      <Icon name="warning" size={12} strokeWidth={2.25} className={styles.bannerIcon} />
      <span className={styles.bannerText}>Diff truncated — the backend caps diffs at 2 MB. Open the file in your editor for the full change.</span>
    </div>
  )
}
