import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { positionAnchored, prefersReducedMotion, stopOverlayKeys, useFocusTrap, useOverlay, type Alignment, type Placement } from './overlays'
import styles from './Popover.module.css'

export interface PopoverProps {
  open: boolean
  /** Esc, an outside press, or the host closing it. */
  onClose: () => void
  /** The element the panel hangs off (a button). Presses on it never count as "outside". */
  anchorRef: RefObject<HTMLElement | null>
  /** Preferred side; flips when the viewport has no room. `below` by default. */
  placement?: Placement
  /** Horizontal alignment with the anchor; `start` by default. */
  align?: Alignment
  /** Panel width (number = px); `auto` by default. */
  width?: number | string
  /** Caps the panel height (it also never exceeds the viewport). */
  maxHeight?: number
  /** Inner padding; none by default (lists go edge to edge). */
  padded?: boolean
  /** Element to focus on open; the first focusable by default, `'none'` to leave focus alone. */
  initialFocus?: RefObject<HTMLElement | null> | 'none'
  /** Give focus back to the anchor on close (default true). */
  returnFocus?: boolean
  role?: 'dialog' | 'listbox' | 'menu' | 'group'
  'aria-label'?: string
  'aria-labelledby'?: string
  className?: string
  style?: CSSProperties
  children: ReactNode
}

interface Box {
  top: number
  left: number
  placement: Placement
  maxHeight: number
}

/** How long a closing popover takes to fade (NSPopover's close, about 0.12 s). */
const LEAVE_MS = 120

/**
 * `.popover(isPresented:arrowEdge:)` of the macOS composer: an anchored
 * floating `surface` panel with a hairline, 12 px radius and the floating
 * shadow, dismissed by Esc or an outside press, focus kept inside while
 * open. Used by the model picker, the permissions and effort menus.
 */
export function Popover({
  open,
  onClose,
  anchorRef,
  placement = 'below',
  align = 'start',
  width,
  maxHeight,
  padded = false,
  initialFocus,
  returnFocus = true,
  role = 'dialog',
  className,
  style,
  children,
  ...aria
}: PopoverProps) {
  const panelRef = useRef<HTMLDivElement>(null)
  const [box, setBox] = useState<Box | null>(null)

  useOverlay({ open, ref: panelRef, anchors: [anchorRef], onDismiss: onClose })
  useFocusTrap({ open, ref: panelRef, initialFocus, returnFocus })

  // Still on screen while it fades out, drawn from what it showed last.
  const [present, setPresent] = useState(open)
  if (open && !present) setPresent(true)
  const shownChildren = useRef(children)
  if (open) shownChildren.current = children
  const leaving = !open && present
  useEffect(() => {
    if (!leaving) return
    if (prefersReducedMotion()) {
      setPresent(false)
      return
    }
    const timer = window.setTimeout(() => setPresent(false), LEAVE_MS)
    return () => window.clearTimeout(timer)
  }, [leaving])

  const reposition = useCallback(() => {
    const anchor = anchorRef.current
    const panel = panelRef.current
    if (!anchor || !panel) return
    const rect = anchor.getBoundingClientRect()
    const size = { width: panel.offsetWidth, height: panel.offsetHeight }
    const position = positionAnchored(rect, size, placement, align)
    setBox((previous) =>
      previous &&
      previous.top === position.top &&
      previous.left === position.left &&
      previous.placement === position.placement &&
      previous.maxHeight === position.maxHeight
        ? previous
        : position,
    )
  }, [anchorRef, placement, align])

  useLayoutEffect(() => {
    if (!present) {
      setBox(null)
      return
    }
    if (open) reposition()
  }, [open, present, reposition, children])

  useEffect(() => {
    if (!open) return
    const panel = panelRef.current
    const onChange = () => reposition()
    window.addEventListener('resize', onChange)
    window.addEventListener('scroll', onChange, true)
    const observer = typeof ResizeObserver !== 'undefined' && panel ? new ResizeObserver(onChange) : null
    if (panel) observer?.observe(panel)
    return () => {
      window.removeEventListener('resize', onChange)
      window.removeEventListener('scroll', onChange, true)
      observer?.disconnect()
    }
  }, [open, reposition])

  if (!open && !present) return null

  const cappedHeight = box ? Math.min(box.maxHeight, maxHeight ?? Infinity) : maxHeight
  const panelStyle: CSSProperties = {
    ...style,
    width,
    top: box?.top ?? -9999,
    left: box?.left ?? -9999,
    maxHeight: cappedHeight,
    visibility: box ? 'visible' : 'hidden',
  }

  return createPortal(
    <div
      ref={panelRef}
      role={role}
      aria-label={aria['aria-label']}
      aria-labelledby={aria['aria-labelledby']}
      tabIndex={-1}
      className={[
        styles.panel,
        padded ? styles.padded : '',
        box?.placement === 'above' ? styles.above : styles.below,
        box && !prefersReducedMotion() ? (leaving ? styles.leave : styles.enter) : '',
        className ?? '',
      ]
        .filter(Boolean)
        .join(' ')}
      style={panelStyle}
      inert={leaving || undefined}
      onKeyDown={stopOverlayKeys}
    >
      {leaving ? shownChildren.current : children}
    </div>,
    document.body,
  )
}
