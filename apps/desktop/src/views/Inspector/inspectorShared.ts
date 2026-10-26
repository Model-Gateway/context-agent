// Small helpers the Inspector tabs share (module-private to views/Inspector).

import { createContext, useContext, useEffect, useState } from 'react'

/// Whether the tab pane is the one the Inspector shows (true) or the one
/// sliding out after a tab switch (false). The Browser tab parks its native
/// page as soon as its pane starts leaving: a native child webview paints
/// above the HTML, so it cannot slide out with the pane.
export const InspectorPaneActiveContext = createContext<boolean>(true)

/** `InspectorPaneActiveContext`'s value. */
export function useInspectorPaneActive(): boolean {
  return useContext(InspectorPaneActiveContext)
}

/// `TimelineView(.periodic(from: .now, by: 1))`: the current time, ticking
/// every `intervalMs` while `enabled` (and not at all otherwise).
export function useNow(enabled: boolean, intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!enabled) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), intervalMs)
    return () => window.clearInterval(timer)
  }, [enabled, intervalMs])
  return enabled ? now : Date.now()
}

/** Joins class names, skipping empty ones. */
export function cx(...names: Array<string | false | null | undefined>): string {
  return names.filter(Boolean).join(' ')
}

/// `ScrollViewReader.scrollTo(id)` in a horizontal tab strip: scrolls `strip`
/// (a positioned scroll container) just enough to show `element` with the
/// strip's own padding beside it. Measured from layout boxes, so a chip that
/// is still scaling in is revealed whole, and no ancestor scrolls with it.
export function revealInStrip(strip: HTMLElement, element: HTMLElement, behavior: ScrollBehavior): void {
  const style = getComputedStyle(strip)
  const padLeft = parseFloat(style.paddingLeft) || 0
  const padRight = parseFloat(style.paddingRight) || 0
  const left = element.offsetLeft - padLeft
  const right = element.offsetLeft + element.offsetWidth + padRight
  if (left < strip.scrollLeft) strip.scrollTo({ left, behavior })
  else if (right > strip.scrollLeft + strip.clientWidth) strip.scrollTo({ left: right - strip.clientWidth, behavior })
}
