import { useEffect, useRef, type RefObject } from 'react'

export interface PageKeyHandlers {
  /** `.onExitCommand` / `.keyboardShortcut(.cancelAction)`: Esc. */
  onEscape?: () => void
  /** `.keyboardShortcut(.defaultAction)`: Return, unless a control that takes Return has the focus. */
  onReturn?: () => void
}

/** Elements that act on Return themselves (a button's own activation, a text area's newline). */
function takesReturn(target: Element): boolean {
  if (target.closest('button, a[href], textarea, select, [role="button"], [role="menuitem"], [contenteditable="true"]')) return true
  return false
}

/**
 * The Apps pages' Esc and Return: `.onExitCommand(perform:)` and the
 * `.cancelAction` / `.defaultAction` buttons of the connect pages. Active
 * while the focus is inside `root` or on nothing (the body), so a key
 * typed in another pane (the inspector's terminal) is left alone; floating
 * layers stop Esc and Return before it (components/overlays.ts).
 */
export function usePageKeys(root: RefObject<HTMLElement | null>, handlers: PageKeyHandlers): void {
  const latest = useRef(handlers)
  useEffect(() => {
    latest.current = handlers
  })

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return
      if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return
      const target = event.target instanceof Element ? event.target : null
      const element = root.current
      const inside = target === null || target === document.body || target === document.documentElement || (element !== null && element.contains(target))
      if (!inside) return
      if (event.key === 'Escape' && latest.current.onEscape) {
        event.preventDefault()
        latest.current.onEscape()
      } else if (event.key === 'Enter' && latest.current.onReturn) {
        if (target && target !== document.body && takesReturn(target)) return
        event.preventDefault()
        latest.current.onReturn()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [root])
}
