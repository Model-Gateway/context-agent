import { useCallback, useEffect, useLayoutEffect, useRef, useState, type FocusEvent, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { formatShortcut } from './platform'
import styles from './Tooltip.module.css'

export interface TooltipProps {
  /** The tooltip text (the `.help()` string in the Swift views). */
  content: ReactNode
  /** An optional shortcut shown after the text, spelled for the platform ("mod+K"). */
  shortcut?: string
  /**
   * Where the bubble goes: below the pointer by default, as AppKit and Windows
   * draw tooltips (or below the element when it came from keyboard focus);
   * `top` puts it above. Flips when there is no room.
   */
  placement?: 'top' | 'bottom'
  /** Hover delay before showing, 400 ms by default. */
  delay?: number
  /** Disables the tooltip (keeps rendering the child). */
  disabled?: boolean
  /** Exactly one element to attach to. */
  children: ReactNode
}

interface Box {
  top: number
  left: number
  placement: 'top' | 'bottom'
}

/** How far below the pointer's hot spot the bubble's top sits (the cursor's height). */
const POINTER_OFFSET = 20

/**
 * `.help(_:)` of the macOS views: a small bubble that appears after a short
 * hover (or keyboard focus) delay below the pointer, rendered through a
 * portal so it is never clipped by a scrolling pane. Pure CSS look, no
 * library. A disabled control still shows its help: it lets the pointer
 * through (`pointer-events: none`), and the tooltip follows the pointer over
 * the control's box from the element under it, in every engine.
 */
export function Tooltip({ content, shortcut, placement = 'bottom', delay = 400, disabled = false, children }: TooltipProps) {
  const anchorRef = useRef<HTMLSpanElement>(null)
  const bubbleRef = useRef<HTMLDivElement>(null)
  const timer = useRef<number | null>(null)
  const [open, setOpen] = useState(false)
  const [box, setBox] = useState<Box | null>(null)
  /** Where the pointer was when the tooltip was asked for; null when it came from keyboard focus. */
  const pointer = useRef<{ x: number; y: number } | null>(null)

  const clear = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current)
      timer.current = null
    }
  }, [])

  /** A press on the element silences the tooltip until the pointer leaves (like AppKit's). */
  const pressed = useRef(false)

  const show = useCallback(() => {
    if (disabled || pressed.current) return
    clear()
    timer.current = window.setTimeout(() => setOpen(true), delay)
  }, [clear, delay, disabled])

  const hide = useCallback(() => {
    clear()
    setOpen(false)
  }, [clear])

  /** The pointer is over the element (its child), for a tooltip re-enabled under it. */
  const hovering = useRef(false)

  const onPointerEnter = useCallback(
    (event: { clientX: number; clientY: number }) => {
      hovering.current = true
      pointer.current = { x: event.clientX, y: event.clientY }
      show()
    },
    [show],
  )

  /** Until the bubble is up it follows the pointer, so it opens where the pointer rests. */
  const onPointerMove = useCallback((event: { clientX: number; clientY: number }) => {
    if (timer.current !== null) pointer.current = { x: event.clientX, y: event.clientY }
  }, [])

  const onPointerLeave = useCallback(() => {
    hovering.current = false
    pressed.current = false
    hide()
  }, [hide])

  const onPointerDown = useCallback(() => {
    pressed.current = true
    hide()
  }, [hide])

  /** Keyboard focus only: a click focuses too, and must not bring the tooltip up. */
  const onFocus = useCallback(
    (event: FocusEvent<HTMLSpanElement>) => {
      const target = event.target as HTMLElement
      let visible = false
      try {
        visible = target.matches(':focus-visible')
      } catch {
        visible = false
      }
      if (visible) {
        pointer.current = null
        show()
      }
    },
    [show],
  )

  // A disabled child gets no pointer events in every engine (it is
  // `pointer-events: none`, see Tooltip.module.css), so its hover is followed
  // from the element under it: inside the child's box counts as over it.
  const [childDisabled, setChildDisabled] = useState(false)
  useLayoutEffect(() => {
    const child = anchorRef.current?.firstElementChild
    const now = child instanceof HTMLElement && child.matches(':disabled')
    if (now !== childDisabled) setChildDisabled(now)
  })
  useEffect(() => {
    if (!childDisabled) return
    const anchor = anchorRef.current
    const child = anchor?.firstElementChild
    const host = anchor?.parentElement
    if (!anchor || !host || !(child instanceof HTMLElement)) return
    let inside = false
    const onMove = (event: PointerEvent) => {
      const rect = child.getBoundingClientRect()
      const now = event.clientX >= rect.left && event.clientX < rect.right && event.clientY >= rect.top && event.clientY < rect.bottom
      if (now === inside) {
        if (now) onPointerMove(event)
        return
      }
      inside = now
      if (now) onPointerEnter(event)
      else onPointerLeave()
    }
    const onLeave = () => {
      if (!inside) return
      inside = false
      onPointerLeave()
    }
    host.addEventListener('pointermove', onMove)
    host.addEventListener('pointerleave', onLeave)
    return () => {
      host.removeEventListener('pointermove', onMove)
      host.removeEventListener('pointerleave', onLeave)
      if (inside) onPointerLeave()
    }
  }, [childDisabled, onPointerEnter, onPointerLeave, onPointerMove])

  useEffect(() => clear, [clear])

  // Disabled: hide. Enabled again while the pointer is still over the element
  // (it left a nested element with its own tooltip): show after the delay.
  useEffect(() => {
    if (disabled) hide()
    else if (hovering.current) show()
  }, [disabled, hide, show])

  // Hide on any press or Esc, like the system tooltip.
  useEffect(() => {
    if (!open) return
    const onDown = () => hide()
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') hide()
    }
    document.addEventListener('pointerdown', onDown, true)
    document.addEventListener('keydown', onKey, true)
    window.addEventListener('scroll', onDown, true)
    return () => {
      document.removeEventListener('pointerdown', onDown, true)
      document.removeEventListener('keydown', onKey, true)
      window.removeEventListener('scroll', onDown, true)
    }
  }, [open, hide])

  useLayoutEffect(() => {
    if (!open) {
      setBox(null)
      return
    }
    const anchor = anchorRef.current?.firstElementChild as HTMLElement | null
    const bubble = bubbleRef.current
    if (!anchor || !bubble) return
    const size = bubble.getBoundingClientRect()
    const margin = 8
    const at = pointer.current
    // Below (or above) the pointer's hot spot, the bubble's left edge at the
    // pointer; from keyboard focus, centred under (or over) the element.
    const rect = at ? { top: at.y - 4, bottom: at.y + POINTER_OFFSET - 6, left: at.x, width: 0 } : anchor.getBoundingClientRect()
    const gap = 6
    let resolved = placement
    if (resolved === 'top' && rect.top - gap - size.height < margin) resolved = 'bottom'
    if (resolved === 'bottom' && rect.bottom + gap + size.height > window.innerHeight - margin) resolved = 'top'
    const top = resolved === 'top' ? rect.top - gap - size.height : rect.bottom + gap
    let left = at ? at.x : rect.left + rect.width / 2 - size.width / 2
    left = Math.min(Math.max(margin, left), Math.max(margin, window.innerWidth - margin - size.width))
    setBox({ top, left, placement: resolved })
  }, [open, placement, content, shortcut])

  return (
    <>
      <span
        ref={anchorRef}
        className={styles.anchor}
        onPointerEnter={onPointerEnter}
        onPointerMove={onPointerMove}
        onPointerLeave={onPointerLeave}
        onPointerDownCapture={onPointerDown}
        onFocusCapture={onFocus}
        onBlurCapture={hide}
      >
        {children}
      </span>
      {open && !disabled
        ? createPortal(
            <div
              ref={bubbleRef}
              role="tooltip"
              className={`${styles.bubble} ${box ? styles.visible : ''} ${box?.placement === 'bottom' ? styles.below : ''}`}
              style={box ? { top: box.top, left: box.left } : { top: -9999, left: -9999 }}
            >
              <span className={styles.text}>{content}</span>
              {shortcut ? <span className={styles.shortcut}>{formatShortcut(shortcut)}</span> : null}
            </div>,
            document.body,
          )
        : null}
    </>
  )
}
