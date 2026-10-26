// Port of `BackendLog` (Backend/BackendProcess.swift): debug lines of the
// protocol layer, off unless asked for. The Swift app reads
// `CTX_DESKTOP_DEBUG`; here the gate is `localStorage['ctx.debug']` (any
// value) or, in a dev build, `window.__CTX_DEBUG = true` from the console.

declare global {
  interface Window {
    /** Set from the devtools console in a dev build to see `[backend]` lines. */
    __CTX_DEBUG?: boolean
  }
}

/** `BackendLog.isEnabled`, evaluated per call so the console switch works at once. */
export function isDebugEnabled(): boolean {
  try {
    if (typeof localStorage !== 'undefined' && localStorage.getItem('ctx.debug') !== null) return true
  } catch {
    // Storage can be unavailable (private mode, blocked); treat as off.
  }
  try {
    return Boolean(import.meta.env.DEV && typeof window !== 'undefined' && window.__CTX_DEBUG)
  } catch {
    return false
  }
}

/// `BackendLog.debug(_:)`: `message` may be a thunk so an expensive line is
/// only built when the gate is on.
export function debug(message: string | (() => string)): void {
  if (!isDebugEnabled()) return
  const text = typeof message === 'function' ? message() : message
  console.debug(`[backend] ${text}`)
}
