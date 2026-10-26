import { useMemo, type ReactNode } from 'react'
import { RenderCache } from '../markdown/renderCache'
import styles from './LineList.module.css'

/**
 * Rendered blocks by highlighter key and text (`LineList.rendered` in
 * Swift, 256 entries): a row that scrolls back into view, or a sibling that
 * toggles, does not tokenize again. Only used when the caller names its
 * highlighter with `cacheKey` (a function's identity is no key).
 */
const rendered = new RenderCache<string, ReactNode>(256)

export interface CappedLines {
  lines: string[]
  /** Lines beyond `maxLines` / `maxBytes` that were not kept. */
  omitted: number
  /** A line longer than the remaining byte budget was cut short. */
  clipped: boolean
}

function utf8Length(text: string): number {
  let bytes = 0
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index)
    if (code < 0x80) bytes += 1
    else if (code < 0x800) bytes += 2
    else if (code >= 0xd800 && code <= 0xdbff) {
      bytes += 4
      index += 1
    } else bytes += 3
  }
  return bytes
}

/** The longest prefix of `text` within `budget` UTF-8 bytes, never inside a code point. */
function clipUtf8(text: string, budget: number): string {
  let bytes = 0
  let index = 0
  while (index < text.length) {
    const code = text.charCodeAt(index)
    let size = 3
    let step = 1
    if (code < 0x80) size = 1
    else if (code < 0x800) size = 2
    else if (code >= 0xd800 && code <= 0xdbff) {
      size = 4
      step = 2
    }
    if (bytes + size > budget) break
    bytes += size
    index += step
  }
  return text.slice(0, index)
}

/**
 * `LineList.cap` of the macOS app: splits `text` into lines and keeps at
 * most `maxLines` lines / `maxBytes` bytes; a line longer than the
 * remaining budget is clipped (never inside a UTF-8 sequence) so one huge
 * line can never reach the renderer. A trailing newline does not count as
 * an extra (empty) line.
 */
export function capLines(text: string, maxLines: number, maxBytes: number): CappedLines {
  const kept: string[] = []
  let budget = Math.max(0, maxBytes)
  let total = 0
  let clipped = false
  let cursor = 0
  const length = text.length
  while (cursor < length) {
    let newline = text.indexOf('\n', cursor)
    if (newline < 0) newline = length
    let lineEnd = newline
    if (lineEnd > cursor && text.charCodeAt(lineEnd - 1) === 0x0d) lineEnd -= 1
    total += 1
    if (kept.length < maxLines && budget > 0) {
      const line = text.slice(cursor, lineEnd)
      const bytes = utf8Length(line)
      if (bytes > budget) {
        kept.push(clipUtf8(line, budget) + '…')
        clipped = true
        budget = 0
      } else {
        kept.push(line)
        budget -= bytes + 1
      }
    }
    cursor = newline === length ? length : newline + 1
  }
  return { lines: kept, omitted: Math.max(0, total - kept.length), clipped }
}

export interface LineListProps {
  text: string
  /** Lines rendered at most; 400 by default. */
  maxLines?: number
  /** Bytes rendered at most; 32 768 by default. */
  maxBytes?: number
  /**
   * Syntax highlighter: the rendered content of one line (spans with token
   * classes from markdown/highlighter.ts). Each line is tokenized on its own.
   */
  highlight?: (line: string, index: number) => ReactNode
  /**
   * Names `highlight` (the language, "json") so the highlighted block is
   * kept in a shared cache across mounts; without it the result is only
   * memoized while this element stays mounted.
   */
  cacheKey?: string
  /** "Open full output" in the footer; without it the footer only counts. */
  onOpenFull?: () => void
  /** Wrap long lines (default); `false` inside a horizontal scroller (CodeBlock). */
  wrap?: boolean
  /** Replaces the default "+ N more lines · Open full output" footer. */
  footer?: ReactNode | ((info: { omitted: number; clipped: boolean }) => ReactNode)
  className?: string
}

/**
 * `LineList` of the macOS app: monospace lines as one `pre` element (not
 * one element per line), optionally syntax-highlighted. Rendering is
 * capped at `maxLines` / `maxBytes`; beyond that a footer says "+ N more
 * lines · Open full output" and calls `onOpenFull`. Selection runs across
 * lines; the capped and highlighted output is memoized.
 */
export function LineList({ text, maxLines = 400, maxBytes = 32_768, highlight, cacheKey, onOpenFull, wrap = true, footer, className }: LineListProps) {
  const capped = useMemo(() => capLines(text, maxLines, maxBytes), [text, maxLines, maxBytes])

  const content = useMemo(() => {
    // An empty line still needs a glyph to keep its height.
    if (!highlight) return capped.lines.map((line) => (line.length === 0 ? ' ' : line)).join('\n')
    const build = () =>
      capped.lines.map((line, index) => (
        <span key={index} className={styles.line}>
          {highlight(line.length === 0 ? ' ' : line, index)}
          {index < capped.lines.length - 1 ? '\n' : null}
        </span>
      ))
    if (cacheKey === undefined) return build()
    // Keyed by the capped lines (at most `maxBytes`), not the whole text.
    return rendered.value(`${cacheKey}\u0000${capped.lines.join('\n')}`, build)
  }, [capped, highlight, cacheKey])

  const needsFooter = capped.omitted > 0 || capped.clipped
  let footerNode: ReactNode = null
  if (needsFooter) {
    if (typeof footer === 'function') footerNode = footer({ omitted: capped.omitted, clipped: capped.clipped })
    else if (footer !== undefined) footerNode = footer
    else {
      const summary = capped.omitted > 0 ? `+ ${capped.omitted.toLocaleString()} more line${capped.omitted === 1 ? '' : 's'}` : 'Long lines clipped'
      footerNode = (
        <button
          type="button"
          className={styles.footer}
          disabled={!onOpenFull}
          onClick={onOpenFull}
          aria-label={onOpenFull ? `${capped.omitted} more lines, open full output` : `${capped.omitted} more lines`}
        >
          <span className={styles.summary}>{summary}</span>
          {onOpenFull ? (
            <>
              <span className={styles.dot}> · </span>
              <span className={styles.open}>Open full output</span>
            </>
          ) : null}
        </button>
      )
    }
  }

  return (
    <div className={[styles.list, className ?? ''].filter(Boolean).join(' ')}>
      <pre className={[styles.pre, wrap ? styles.wrap : styles.nowrap, 'selectable'].join(' ')}>{content}</pre>
      {footerNode}
    </div>
  )
}
