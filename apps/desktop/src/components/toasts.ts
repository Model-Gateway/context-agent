import { create } from 'zustand'
import { showAlert } from './dialogs'

/**
 * `Toast` + `ToastCenter` of the macOS app (App/Toasts.swift): transient
 * messages stacked in the window's bottom-right corner (`ToastOverlay`). Identical messages are deduped
 * within 10 s (the visible copy's timer is refreshed instead), every toast
 * auto-dismisses after 5 s, at most 4 are visible, and "Details" routes
 * through `onDetails` (an alert with a Copy button by default).
 */

export type ToastLevel = 'info' | 'success' | 'warning' | 'error'

export interface Toast {
  id: string
  message: string
  level: ToastLevel
  /** Longer text behind the "Details" action (stderr tail, RPC message…). */
  details?: string
  date: number
}

export interface ToastInput {
  message: string
  level?: ToastLevel
  details?: string
}

export const TOAST_DEDUPE_INTERVAL = 10_000
export const TOAST_AUTO_DISMISS_INTERVAL = 5_000
export const TOAST_MAX_VISIBLE = 4

interface ToastState {
  toasts: Toast[]
  /** Shows a toast unless the same message was shown within the last 10 s. */
  push: (input: ToastInput) => void
  dismiss: (id: string) => void
  dismissAll: () => void
  /** The "Details" action: hands the toast to `onDetails` and dismisses it. */
  showDetails: (toast: Toast) => void
  /** Replace what "Details" does (AppState shows an alert by default). */
  setOnDetails: (handler: ((toast: Toast) => void) | null) => void
}

const lastShown = new Map<string, number>()
const timers = new Map<string, number>()
let onDetails: ((toast: Toast) => void) | null = null
let counter = 0

function defaultOnDetails(toast: Toast) {
  void showAlert({ title: toast.message, details: toast.details })
}

export const useToasts = create<ToastState>((set, get) => {
  const schedule = (id: string) => {
    const existing = timers.get(id)
    if (existing !== undefined) window.clearTimeout(existing)
    timers.set(
      id,
      window.setTimeout(() => {
        timers.delete(id)
        get().dismiss(id)
      }, TOAST_AUTO_DISMISS_INTERVAL),
    )
  }

  const cancel = (id: string) => {
    const existing = timers.get(id)
    if (existing !== undefined) window.clearTimeout(existing)
    timers.delete(id)
  }

  return {
    toasts: [],

    push: ({ message, level = 'info', details }) => {
      const trimmed = message.trim()
      if (!trimmed) return
      const now = Date.now()
      const last = lastShown.get(trimmed)
      if (last !== undefined && now - last < TOAST_DEDUPE_INTERVAL) {
        // Refresh the visible copy's timer instead of stacking a duplicate.
        const existing = get().toasts.find((toast) => toast.message === trimmed)
        if (existing) schedule(existing.id)
        return
      }
      lastShown.set(trimmed, now)
      if (lastShown.size > 64) {
        for (const [key, at] of lastShown) {
          if (now - at >= TOAST_DEDUPE_INTERVAL) lastShown.delete(key)
        }
      }
      const toast: Toast = { id: `toast-${++counter}`, message: trimmed, level, details: details || undefined, date: now }
      const toasts = [...get().toasts, toast]
      while (toasts.length > TOAST_MAX_VISIBLE) {
        const dropped = toasts.shift()
        if (dropped) cancel(dropped.id)
      }
      set({ toasts })
      schedule(toast.id)
    },

    dismiss: (id) => {
      cancel(id)
      set((state) => (state.toasts.some((toast) => toast.id === id) ? { toasts: state.toasts.filter((toast) => toast.id !== id) } : state))
    },

    dismissAll: () => {
      for (const id of timers.keys()) cancel(id)
      set({ toasts: [] })
    },

    showDetails: (toast) => {
      ;(onDetails ?? defaultOnDetails)(toast)
      get().dismiss(toast.id)
    },

    setOnDetails: (handler) => {
      onDetails = handler
    },
  }
})

/** The store's actions without a hook, for stores and the backend layer. */
export const toasts = {
  push: (input: ToastInput) => useToasts.getState().push(input),
  dismiss: (id: string) => useToasts.getState().dismiss(id),
  dismissAll: () => useToasts.getState().dismissAll(),
  showDetails: (toast: Toast) => useToasts.getState().showDetails(toast),
  setOnDetails: (handler: ((toast: Toast) => void) | null) => useToasts.getState().setOnDetails(handler),
}
