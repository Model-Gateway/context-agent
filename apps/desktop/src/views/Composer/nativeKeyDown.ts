// The composer popovers' Return key. The foundation's focus trap
// (components/overlays.ts `useFocusTrap`) stops Return and Esc at the
// popover's panel with a native listener, before they bubble to React's
// listener on the portal's container (document.body), so a React
// `onKeyDown` inside a `Popover` never sees them. A native listener on an
// element inside the panel runs first; CommandPalette does the same.

import { useEffect, useLayoutEffect, useRef, type RefObject } from 'react'

/** Calls `handler` for every native `keydown` on `ref`'s element (attached once it mounts). */
export function useNativeKeyDown(ref: RefObject<HTMLElement | null>, handler: (event: KeyboardEvent) => void): void {
  const latest = useRef(handler)
  useLayoutEffect(() => {
    latest.current = handler
  })
  useEffect(() => {
    const element = ref.current
    if (!element) return
    const listener = (event: KeyboardEvent) => latest.current(event)
    element.addEventListener('keydown', listener)
    return () => element.removeEventListener('keydown', listener)
  }, [ref])
}

/** Return (or Enter) with no modifier and no IME composition in progress. */
export function isPlainReturn(event: KeyboardEvent): boolean {
  return (
    event.key === 'Enter' &&
    !event.isComposing &&
    event.keyCode !== 229 &&
    !event.shiftKey &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey
  )
}
