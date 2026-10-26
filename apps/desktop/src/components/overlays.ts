import { useEffect, useLayoutEffect, useRef, type RefObject } from 'react'

/**
 * Shared plumbing for the floating layers (Popover, Menu, Sheet): a stack
 * of open overlays so Esc closes only the topmost one and a click inside a
 * nested overlay never dismisses the one under it, plus the focus trap and
 * the "Esc / Return inside popovers never reach the composer" rule from
 * ARCHITECTURE.md (the global key handler in state/keys.ts listens in the
 * bubbling phase; overlays stop those keys before it).
 */

interface OverlayEntry {
  id: number
  element: () => HTMLElement | null
  anchors: () => Array<HTMLElement | null | undefined>
  onDismiss: () => void
  modal: boolean
}

const stack: OverlayEntry[] = []
let nextId = 1
const stackListeners = new Set<() => void>()

function stackChanged(): void {
  for (const listener of [...stackListeners]) listener()
}

/**
 * Subscribes to overlays opening and closing (the Inspector Browser tab
 * parks its native page while an overlay covers it, since a native child
 * webview always paints above the HTML). Returns the unsubscribe function.
 */
export function onOverlayStackChange(listener: () => void): () => void {
  stackListeners.add(listener)
  return () => {
    stackListeners.delete(listener)
  }
}

/** The root elements of the open overlays (Popover, Menu, Sheet), bottom first. */
export function openOverlayElements(): HTMLElement[] {
  return stack.map((entry) => entry.element()).filter((element): element is HTMLElement => element !== null)
}

function topmost(): OverlayEntry | undefined {
  return stack[stack.length - 1]
}

function isInside(target: Node | null, element: HTMLElement | null | undefined): boolean {
  return !!element && !!target && element.contains(target)
}

/** The overlays above `entry` in the stack. */
function above(entry: OverlayEntry): OverlayEntry[] {
  const index = stack.indexOf(entry)
  return index < 0 ? [] : stack.slice(index + 1)
}

let listenersInstalled = false

function onKeyDown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  const top = topmost()
  if (!top) return
  event.preventDefault()
  event.stopPropagation()
  top.onDismiss()
}

function onPointerDown(event: PointerEvent) {
  const target = event.target as Node | null
  // Walk from the top: an overlay is dismissed by a press outside it, its
  // anchors and every overlay above it; modal overlays swallow the press.
  for (let index = stack.length - 1; index >= 0; index -= 1) {
    const entry = stack[index]
    if (!entry) continue
    const inside =
      isInside(target, entry.element()) ||
      entry.anchors().some((anchor) => isInside(target, anchor)) ||
      above(entry).some((other) => isInside(target, other.element()))
    if (inside) return
    entry.onDismiss()
    if (entry.modal) return
  }
}

function installListeners() {
  if (listenersInstalled || typeof document === 'undefined') return
  listenersInstalled = true
  document.addEventListener('keydown', onKeyDown, true)
  document.addEventListener('pointerdown', onPointerDown, true)
}

export interface UseOverlayOptions {
  /** Whether the overlay is shown; registers while true. */
  open: boolean
  /** The overlay's root element. */
  ref: RefObject<HTMLElement | null>
  /** Elements that count as "inside" for outside-click (the anchor button). */
  anchors?: Array<RefObject<HTMLElement | null>>
  /** Called on Esc or an outside press. */
  onDismiss: () => void
  /** A modal overlay (Sheet) stops an outside press from reaching anything beneath. */
  modal?: boolean
}

/**
 * Registers an overlay on the stack while `open`: Esc (document capture)
 * dismisses the topmost, an outside pointer-down dismisses every overlay it
 * is outside of (top first), and `anchors` are treated as inside.
 */
export function useOverlay({ open, ref, anchors = [], onDismiss, modal = false }: UseOverlayOptions): void {
  const dismissRef = useRef(onDismiss)
  const anchorsRef = useRef(anchors)
  // The latest callback and anchors, without re-registering the overlay.
  useLayoutEffect(() => {
    dismissRef.current = onDismiss
    anchorsRef.current = anchors
  })

  useEffect(() => {
    if (!open) return
    installListeners()
    const entry: OverlayEntry = {
      id: nextId++,
      element: () => ref.current,
      anchors: () => anchorsRef.current.map((anchor) => anchor.current),
      onDismiss: () => dismissRef.current(),
      modal,
    }
    stack.push(entry)
    stackChanged()
    return () => {
      const index = stack.indexOf(entry)
      if (index >= 0) stack.splice(index, 1)
      stackChanged()
    }
  }, [open, ref, modal])
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"]):not([disabled]), [contenteditable="true"]'

/** The keyboard-focusable descendants of `root`, in DOM order. */
export function focusableElements(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (element) => element.offsetParent !== null || element === document.activeElement,
  )
}

export interface UseFocusTrapOptions {
  open: boolean
  ref: RefObject<HTMLElement | null>
  /** The element to focus on open; defaults to the first focusable, else the root. */
  initialFocus?: RefObject<HTMLElement | null> | 'none'
  /** Give focus back to where it was when the overlay closes (default true). */
  returnFocus?: boolean
}

/**
 * Keeps Tab / Shift+Tab inside the overlay while it is open, moves focus in
 * on open and returns it on close. Esc and Enter stop at the overlay so the
 * composer and the global shortcuts never see them.
 */
export function useFocusTrap({ open, ref, initialFocus, returnFocus = true }: UseFocusTrapOptions): void {
  const previous = useRef<HTMLElement | null>(null)

  useLayoutEffect(() => {
    if (!open) return
    const root = ref.current
    if (!root) return
    previous.current = (document.activeElement as HTMLElement | null) ?? null
    if (initialFocus !== 'none') {
      const target = initialFocus?.current ?? focusableElements(root)[0] ?? root
      // Let the element land in the DOM at its final position first.
      const handle = requestAnimationFrame(() => target.focus({ preventScroll: true }))
      return () => cancelAnimationFrame(handle)
    }
    return undefined
    // initialFocus is a ref (or the constant 'none'): its identity is stable.
  }, [open, ref, initialFocus])

  useEffect(() => {
    if (!open) return
    const root = ref.current
    if (!root) return
    // Esc and Return never reach the composer or the global shortcuts
    // (state/keys.ts listens on window): they stop at the document, after
    // React's listener on the portal's container (document.body) has run, so
    // an `onKeyDown` inside the overlay still sees them.
    const onBubble = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' && event.key !== 'Enter') return
      const target = event.target
      if (target instanceof Node && root.contains(target)) event.stopPropagation()
    }
    document.addEventListener('keydown', onBubble)
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return
      const focusables = focusableElements(root)
      if (focusables.length === 0) {
        event.preventDefault()
        root.focus()
        return
      }
      const first = focusables[0]
      const last = focusables[focusables.length - 1]
      const active = document.activeElement as HTMLElement | null
      if (event.shiftKey && (active === first || !root.contains(active))) {
        event.preventDefault()
        last?.focus()
      } else if (!event.shiftKey && (active === last || !root.contains(active))) {
        event.preventDefault()
        first?.focus()
      }
    }
    root.addEventListener('keydown', onKey)
    return () => {
      root.removeEventListener('keydown', onKey)
      document.removeEventListener('keydown', onBubble)
      if (returnFocus) {
        const target = previous.current
        if (target && document.contains(target)) restoreFocus(target)
      }
    }
  }, [open, ref, returnFocus])
}

/**
 * `onKeyDown` for an overlay's portal root: Esc and Return have reached
 * every handler inside the overlay by now; they stop here so they never
 * bubble on to the React ancestors outside it (a row that opens a chat on
 * Return, the view the overlay was declared in).
 */
export function stopOverlayKeys(event: { key: string; stopPropagation: () => void }): void {
  if (event.key === 'Escape' || event.key === 'Enter') event.stopPropagation()
}

/**
 * Gives focus back to `target` after an overlay closes, without the
 * keyboard focus ring: macOS draws none on the control a menu, popover or
 * sheet returns to, while Chromium shows one when the overlay was closed
 * with a key (Esc, Return). The ring comes back with the next navigation key.
 */
export function restoreFocus(target: HTMLElement): void {
  target.setAttribute('data-quiet-focus', '')
  const clear = () => {
    target.removeAttribute('data-quiet-focus')
    target.removeEventListener('blur', clear)
    document.removeEventListener('keydown', onKey, true)
  }
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Tab' || event.key.startsWith('Arrow')) clear()
  }
  target.addEventListener('blur', clear)
  document.addEventListener('keydown', onKey, true)
  target.focus({ preventScroll: true })
}

export type Placement = 'below' | 'above'
export type Alignment = 'start' | 'center' | 'end'

export interface AnchoredPosition {
  top: number
  left: number
  placement: Placement
  maxHeight: number
}

/**
 * Where to put a floating panel of `size` next to `anchor`, preferring
 * `placement` and flipping when the viewport has no room, kept 8 px inside
 * the window.
 */
export function positionAnchored(
  anchor: DOMRect,
  size: { width: number; height: number },
  placement: Placement,
  align: Alignment,
  gap = 6,
  margin = 8,
): AnchoredPosition {
  const viewportWidth = window.innerWidth
  const viewportHeight = window.innerHeight
  const roomBelow = viewportHeight - anchor.bottom - margin
  const roomAbove = anchor.top - margin
  let resolved: Placement = placement
  if (placement === 'below' && size.height + gap > roomBelow && roomAbove > roomBelow) resolved = 'above'
  if (placement === 'above' && size.height + gap > roomAbove && roomBelow > roomAbove) resolved = 'below'
  const top = resolved === 'below' ? anchor.bottom + gap : anchor.top - gap - size.height
  const maxHeight = Math.max(80, (resolved === 'below' ? roomBelow : roomAbove) - gap)
  let left = anchor.left
  if (align === 'center') left = anchor.left + anchor.width / 2 - size.width / 2
  if (align === 'end') left = anchor.right - size.width
  left = Math.min(Math.max(margin, left), Math.max(margin, viewportWidth - margin - size.width))
  return { top: Math.max(margin, top), left, placement: resolved, maxHeight }
}

/**
 * Where to put a context menu opened at a point: below-right of it, moved
 * up or left when it would leave the window.
 */
export function positionAtPoint(point: { x: number; y: number }, size: { width: number; height: number }, margin = 8): AnchoredPosition {
  const viewportWidth = window.innerWidth
  const viewportHeight = window.innerHeight
  let left = point.x
  let top = point.y
  if (left + size.width + margin > viewportWidth) left = Math.max(margin, point.x - size.width)
  if (top + size.height + margin > viewportHeight) top = Math.max(margin, viewportHeight - margin - size.height)
  return { top, left, placement: 'below', maxHeight: viewportHeight - margin * 2 }
}

/** True when the user prefers reduced motion (the Reduce Motion setting). */
export function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches
}
