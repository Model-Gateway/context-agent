import { useEffect, useId, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { prefersReducedMotion, stopOverlayKeys, useFocusTrap, useOverlay } from './overlays'
import styles from './Sheet.module.css'

/** The promise-returning alert helpers live in dialogs.tsx; re-exported here for callers that look for them by the Sheet. */
export { confirm, showAlert, type AlertOptions, type ConfirmOptions } from './dialogs'

export interface SheetProps {
  open: boolean
  /** Esc or a press on the backdrop: the cancel path. */
  onClose: () => void
  title?: ReactNode
  /** The body. */
  children?: ReactNode
  /** The button row, laid out right-aligned (Cancel before the primary action). */
  footer?: ReactNode
  /** Card width in px; 440 by default. */
  width?: number
  /** Element to focus on open; the first focusable by default. */
  initialFocus?: RefObject<HTMLElement | null> | 'none'
  /** A press on the dimmed backdrop cancels (default true). */
  dismissOnBackdrop?: boolean
  /** Accessible name when there is no title. */
  'aria-label'?: string
  className?: string
  style?: CSSProperties
}

/** How long a closing sheet takes to slide back up (SwiftUI's sheet dismissal, about 0.15 s). */
const LEAVE_MS = 150

/**
 * `.sheet(isPresented:)` / `.alert` of the macOS app: a modal `surface`
 * card that drops from the bottom of the top bar (macOS sheets hang from the
 * window's title bar) over a dimmed backdrop, with a title, body and a
 * right-aligned button row; Esc cancels. Closing slides it back up, drawn
 * from its last content. Used by the rewind, push and commit sheets and by
 * the `confirm()` / `showAlert()` helpers in dialogs.tsx.
 */
export function Sheet({ open, onClose, title, children, footer, width = 440, initialFocus, dismissOnBackdrop = true, className, style, ...aria }: SheetProps) {
  const backdropRef = useRef<HTMLDivElement>(null)
  const cardRef = useRef<HTMLDivElement>(null)
  const titleId = useId()

  // The backdrop covers the window, so the overlay stack only routes Esc
  // here; presses on the backdrop are this component's own decision.
  useOverlay({ open, ref: backdropRef, onDismiss: onClose, modal: true })
  useFocusTrap({ open, ref: cardRef, initialFocus })

  // Still on screen while it leaves, drawn from what it showed last.
  const [present, setPresent] = useState(open)
  if (open && !present) setPresent(true)
  const shown = useRef({ title, children, footer, width, className, style })
  if (open) shown.current = { title, children, footer, width, className, style }
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

  if (!open && !present) return null
  const animate = !prefersReducedMotion()
  const view = shown.current

  return createPortal(
    <div
      ref={backdropRef}
      className={[styles.backdrop, animate ? (leaving ? styles.fadeOut : styles.fadeIn) : ''].filter(Boolean).join(' ')}
      inert={leaving || undefined}
      onPointerDown={(event) => {
        if (!dismissOnBackdrop || leaving) return
        if (event.target === event.currentTarget) {
          event.preventDefault()
          onClose()
        }
      }}
    >
      <div
        ref={cardRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={view.title ? titleId : undefined}
        aria-label={view.title ? undefined : aria['aria-label']}
        tabIndex={-1}
        className={[styles.card, animate ? (leaving ? styles.leave : styles.enter) : '', view.className ?? ''].filter(Boolean).join(' ')}
        style={{ width: view.width, ...view.style }}
        onPointerDown={(event) => event.stopPropagation()}
        onKeyDown={stopOverlayKeys}
      >
        {view.title ? (
          <div id={titleId} className={styles.title}>
            {view.title}
          </div>
        ) : null}
        {view.children !== undefined && view.children !== null ? <div className={styles.body}>{view.children}</div> : null}
        {view.footer ? <div className={styles.footer}>{view.footer}</div> : null}
      </div>
    </div>,
    document.body,
  )
}
