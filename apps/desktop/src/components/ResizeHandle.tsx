import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'
import styles from './ResizeHandle.module.css'

export interface ResizeHandleProps {
  /** The pane's current width. */
  width: number
  min: number
  max: number
  /** Restored by a double click. */
  defaultWidth: number
  onResize: (width: number) => void
  /**
   * Which edge of the pane the handle sits on: `leading` (the inspector's
   * left edge: dragging left widens) or `trailing` (dragging right widens).
   */
  edge?: 'leading' | 'trailing'
  /** Accessible name; "Resize pane" by default. */
  label?: string
  className?: string
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

/**
 * `PaneResizeHandle` of the macOS MainWindow: an invisible 8 px strip over
 * a pane's hairline; dragging resizes the pane between `min` and `max`,
 * double-clicking restores `defaultWidth`. Also a keyboard separator:
 * arrows move it 8 px (32 with Shift), Home / End go to the limits.
 */
export function ResizeHandle({ width, min, max, defaultWidth, onResize, edge = 'leading', label = 'Resize pane', className }: ResizeHandleProps) {
  const start = useRef<{ x: number; width: number } | null>(null)
  const [dragging, setDragging] = useState(false)

  useEffect(() => {
    if (!dragging) return
    const previous = document.body.style.cursor
    document.body.style.cursor = 'col-resize'
    return () => {
      document.body.style.cursor = previous
    }
  }, [dragging])

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    start.current = { x: event.clientX, width }
    setDragging(true)
  }

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const origin = start.current
    if (!origin) return
    const delta = event.clientX - origin.x
    const next = edge === 'leading' ? origin.width - delta : origin.width + delta
    onResize(clamp(Math.round(next), min, max))
  }

  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (!start.current) return
    start.current = null
    setDragging(false)
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 32 : 8
    // Left shrinks a leading-edge pane? No: the handle moves with the arrow,
    // so Left widens a pane whose handle is on its left edge.
    const sign = edge === 'leading' ? -1 : 1
    let next: number | null = null
    switch (event.key) {
      case 'ArrowLeft':
        next = width - sign * step
        break
      case 'ArrowRight':
        next = width + sign * step
        break
      case 'Home':
        next = min
        break
      case 'End':
        next = max
        break
      case 'Enter':
        next = defaultWidth
        break
      default:
        return
    }
    event.preventDefault()
    onResize(clamp(next, min, max))
  }

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={width}
      tabIndex={0}
      className={[styles.handle, dragging ? styles.dragging : '', className ?? ''].filter(Boolean).join(' ')}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={() => onResize(clamp(defaultWidth, min, max))}
      onKeyDown={onKeyDown}
    />
  )
}
