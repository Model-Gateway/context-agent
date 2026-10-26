import { useLayoutEffect, useRef, useState, type ClipboardEvent, type CSSProperties, type ReactNode } from 'react'
import styles from './MiddleTruncate.module.css'

/** A run of styled text (the model picker's highlighted matches); the runs' texts concatenate to `text`. */
export interface TextRun {
  text: string
  className?: string
}

export interface MiddleTruncateProps {
  /** The full text; also the accessible name. */
  text: string
  className?: string
  style?: CSSProperties
  /**
   * Hug the text like a SwiftUI `Text` in an `HStack`: the element is as wide
   * as the whole text (a hidden sizer sets its basis) and gives way, dropping
   * characters from the middle, when the row is narrower. Without it the
   * width comes from the container (a block, or a flex item that grows).
   */
  natural?: boolean
  /** With `natural`: caps the natural width (`.frame(maxWidth:)`). */
  maxWidth?: number
  /** Styled pieces of `text`, cut at the same characters as the plain text. */
  runs?: readonly TextRun[]
  /**
   * `.lineLimit(n)` with `.truncationMode(.middle)`: the text wraps onto at
   * most `lines` lines, and characters leave its middle until it fits them
   * (measured on a hidden copy, since the fit depends on where lines break).
   */
  lines?: number
}

let measureContext: CanvasRenderingContext2D | null = null

/** Width of `text` in `font` (a computed `font` shorthand). */
function measure(text: string, font: string): number {
  if (measureContext === null) measureContext = document.createElement('canvas').getContext('2d')
  if (measureContext === null) return 0
  if (measureContext.font !== font) measureContext.font = font
  return measureContext.measureText(text).width
}

/// How many characters of `text` stay at its head and tail around the "…"
/// so it fits `width` (the head keeps one character more than the tail,
/// like AppKit's middle truncation); null when the whole text fits.
function middleCut(text: string, width: number, font: string): { head: number; tail: number } | null {
  if (measure(text, font) <= width + 0.5) return null
  const chars = [...text]
  let low = 0
  let high = chars.length - 1
  while (low < high) {
    const mid = (low + high + 1) >> 1
    const head = Math.ceil(mid / 2)
    const candidate = chars.slice(0, head).join('') + '…' + chars.slice(chars.length - (mid - head)).join('')
    if (measure(candidate, font) <= width + 0.5) low = mid
    else high = mid - 1
  }
  const head = Math.ceil(low / 2)
  return { head, tail: low - head }
}

/// The longest "head…tail" form of `text` that fits `width` in `font` (a
/// computed `font`). Exported for views whose available width is not the
/// text element's own (a path followed by buttons that hug it).
export function middleTruncated(text: string, width: number, font: string): string {
  const cut = middleCut(text, width, font)
  if (cut === null) return text
  const chars = [...text]
  return chars.slice(0, cut.head).join('') + '…' + chars.slice(chars.length - cut.tail).join('')
}

/** The runs between character offsets `from` and `to` of their concatenation. */
function sliceRuns(runs: readonly TextRun[], from: number, to: number): TextRun[] {
  const out: TextRun[] = []
  let index = 0
  for (const run of runs) {
    const chars = [...run.text]
    const start = index
    const end = index + chars.length
    index = end
    const a = Math.max(from, start)
    const b = Math.min(to, end)
    if (a < b) out.push({ text: chars.slice(a - start, b - start).join(''), className: run.className })
  }
  return out
}

/// The multi-line fit: the most characters (head and tail around "…") whose
/// wrapped text still fits the element's line clamp, measured on a hidden
/// copy of the element at its current width. Returns the effect cleanup.
function fitLines(element: HTMLElement, text: string, setCut: (cut: { head: number; tail: number } | null) => void): () => void {
  let lastWidth = -1
  const fit = () => {
    const parent = element.parentElement
    const width = element.getBoundingClientRect().width
    if (!parent || width <= 0 || Math.abs(width - lastWidth) < 0.5) return
    lastWidth = width
    const probe = element.cloneNode(false) as HTMLElement
    probe.removeAttribute('aria-label')
    probe.setAttribute('aria-hidden', 'true')
    Object.assign(probe.style, { position: 'absolute', visibility: 'hidden', pointerEvents: 'none', left: '0', top: '0', width: `${width}px` })
    parent.appendChild(probe)
    const fits = (candidate: string) => {
      probe.textContent = candidate
      return probe.scrollHeight <= probe.clientHeight + 1
    }
    const chars = [...text]
    let result: { head: number; tail: number } | null = null
    if (!fits(text)) {
      let low = 0
      let high = chars.length - 1
      while (low < high) {
        const mid = Math.ceil((low + high) / 2)
        const head = Math.ceil(mid / 2)
        if (fits(`${chars.slice(0, head).join('')}…${chars.slice(chars.length - (mid - head)).join('')}`)) low = mid
        else high = mid - 1
      }
      const head = Math.ceil(low / 2)
      result = { head, tail: low - head }
    }
    probe.remove()
    setCut(result)
  }
  fit()
  const observer = new ResizeObserver(fit)
  observer.observe(element)
  return () => observer.disconnect()
}

/**
 * `Text(…).lineLimit(1).truncationMode(.middle)` of the macOS views: one
 * line that drops characters from its middle ("~/Projects/…/context-agent")
 * when it does not fit. CSS only truncates at the end, so the visible text
 * is fitted with a canvas measurement whenever the element's width changes.
 * The full text stays the accessible name, and copying the whole shortened
 * text copies the whole text, as AppKit does. Every middle-truncated text in
 * the app goes through this component.
 */
export function MiddleTruncate({ text, className, style, natural = false, maxWidth, runs, lines = 1 }: MiddleTruncateProps) {
  const ref = useRef<HTMLSpanElement>(null)
  const [cut, setCut] = useState<{ head: number; tail: number } | null>(null)

  useLayoutEffect(() => {
    const element = ref.current
    if (!element) return
    if (lines > 1) return fitLines(element, text, setCut)
    const fit = () => {
      const computed = getComputedStyle(element)
      const width = element.clientWidth - (parseFloat(computed.paddingLeft) || 0) - (parseFloat(computed.paddingRight) || 0)
      if (width <= 0) {
        setCut(null)
        return
      }
      const next = middleCut(text, width, computed.font)
      setCut((current) => (current === next || (current !== null && next !== null && current.head === next.head && current.tail === next.tail) ? current : next))
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(element)
    return () => observer.disconnect()
  }, [text, lines])

  const chars = cut === null ? null : [...text]
  const shown = cut === null || chars === null ? text : chars.slice(0, cut.head).join('') + '…' + chars.slice(chars.length - cut.tail).join('')
  let content: ReactNode = shown
  if (runs) {
    const render = (list: readonly TextRun[], prefix: string) =>
      list.map((run, i) => (
        <span key={`${prefix}${i}`} className={run.className}>
          {run.text}
        </span>
      ))
    content =
      cut === null || chars === null ? (
        render(runs, 'r')
      ) : (
        <>
          {render(sliceRuns(runs, 0, cut.head), 'h')}…{render(sliceRuns(runs, chars.length - cut.tail, chars.length), 't')}
        </>
      )
  }

  const onCopy = (event: ClipboardEvent<HTMLSpanElement>) => {
    // Selecting the whole shortened text copies the whole text.
    if (shown === text || window.getSelection()?.toString() !== shown) return
    event.clipboardData.setData('text/plain', text)
    event.preventDefault()
  }

  const multiline = lines > 1
  const lineStyle = multiline ? ({ ['--middle-lines' as string]: lines } as CSSProperties) : undefined
  const fitted = (
    <span
      ref={ref}
      className={[multiline ? styles.lines : styles.text, natural ? styles.fitted : className ?? ''].filter(Boolean).join(' ')}
      style={natural ? lineStyle : { ...lineStyle, ...style }}
      aria-label={shown === text ? undefined : text}
      onCopy={onCopy}
    >
      {content}
    </span>
  )
  if (!natural) return fitted
  return (
    <span className={[styles.natural, className ?? ''].filter(Boolean).join(' ')} style={maxWidth !== undefined ? { ...style, maxWidth } : style}>
      {/* The sizer draws the text as generated content, so it is neither
          found, selected nor copied twice. */}
      <span className={styles.sizer} data-text={text} aria-hidden="true" />
      {fitted}
    </span>
  )
}
