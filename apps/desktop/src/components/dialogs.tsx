import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { Button } from './Button'
import { CopyButton } from './CopyButton'
import { Sheet } from './Sheet'
import styles from './dialogs.module.css'

/**
 * `confirm()` and `showAlert()`: the macOS app's `.alert` and
 * `.confirmationDialog` as promises, rendered as a `Sheet` in a root of
 * their own under `document.body` so any code (stores, views) can ask
 * without mounting a host.
 */

export interface ConfirmOptions {
  title: string
  message?: string
  /** The primary button; "OK" by default. */
  confirmLabel?: string
  /** "Cancel" by default. */
  cancelLabel?: string
  /** A destructive primary button (delete, discard, remove). */
  destructive?: boolean
}

export interface AlertOptions {
  title: string
  message?: string
  /** Longer text (a stderr tail, an RPC message) in a monospace box with a Copy button. */
  details?: string
  /** "OK" by default. */
  okLabel?: string
}

type Request =
  | { kind: 'confirm'; id: number; options: ConfirmOptions; resolve: (value: boolean) => void }
  | { kind: 'alert'; id: number; options: AlertOptions; resolve: () => void }

let queue: Request[] = []
let nextId = 1
const listeners = new Set<() => void>()
let root: Root | null = null

function emit() {
  listeners.forEach((listener) => listener())
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function snapshot(): Request[] {
  return queue
}

function ensureRoot() {
  if (root || typeof document === 'undefined') return
  const container = document.createElement('div')
  container.setAttribute('data-ctx-dialogs', '')
  document.body.appendChild(container)
  root = createRoot(container)
  root.render(<DialogHost />)
}

function enqueue(request: Request) {
  queue = [...queue, request]
  ensureRoot()
  emit()
}

function finish(id: number) {
  queue = queue.filter((request) => request.id !== id)
  emit()
}

/** Asks a yes / no question; resolves true for the primary button. */
export function confirm(options: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => enqueue({ kind: 'confirm', id: nextId++, options, resolve }))
}

/** Shows a message (toast "Details", errors) with an OK button; resolves when dismissed. */
export function showAlert(options: AlertOptions): Promise<void> {
  return new Promise((resolve) => enqueue({ kind: 'alert', id: nextId++, options, resolve }))
}

/** How long an answered dialog stays mounted while its sheet slides away (Sheet's own leave). */
const LEAVE_MS = 160

function DialogHost() {
  const requests = useSyncExternalStore(subscribe, snapshot, snapshot)
  const current = requests[0]
  // The dialog just answered stays mounted, closed, while its sheet leaves.
  const [leaving, setLeaving] = useState<Request | null>(null)
  const [last, setLast] = useState<Request | undefined>(current)
  if (last !== current) {
    setLast(current)
    if (last) setLeaving(last)
  }
  useEffect(() => {
    if (!leaving) return
    const timer = window.setTimeout(() => setLeaving(null), LEAVE_MS)
    return () => window.clearTimeout(timer)
  }, [leaving])
  const render = (request: Request, open: boolean) =>
    request.kind === 'confirm' ? <ConfirmSheet key={request.id} request={request} open={open} /> : <AlertSheet key={request.id} request={request} open={open} />
  // One keyed list, so the answered dialog keeps its Sheet instance (and
  // plays its leave) when it moves out of the current slot.
  const shown = [leaving !== current ? leaving : null, current ?? null].filter((request): request is Request => request !== null)
  return <>{shown.map((request) => render(request, request === current))}</>
}

function ConfirmSheet({ request, open }: { request: Extract<Request, { kind: 'confirm' }>; open: boolean }) {
  const primaryRef = useRef<HTMLButtonElement>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const { title, message, confirmLabel = 'OK', cancelLabel = 'Cancel', destructive = false } = request.options
  const settle = (value: boolean) => {
    finish(request.id)
    request.resolve(value)
  }
  return (
    <Sheet
      open={open}
      onClose={() => settle(false)}
      title={title}
      width={420}
      // A destructive action is never the Return default (macOS alerts and
      // confirmation dialogs leave it to a click); Return cancels instead.
      initialFocus={destructive ? cancelRef : primaryRef}
      footer={
        <>
          <Button ref={cancelRef} variant="bordered" onClick={() => settle(false)}>
            {cancelLabel}
          </Button>
          <Button ref={primaryRef} variant={destructive ? 'destructive' : 'prominent'} onClick={() => settle(true)}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {message ? <p className={styles.message}>{message}</p> : null}
    </Sheet>
  )
}

function AlertSheet({ request, open }: { request: Extract<Request, { kind: 'alert' }>; open: boolean }) {
  const okRef = useRef<HTMLButtonElement>(null)
  const { title, message, details, okLabel = 'OK' } = request.options
  const settle = () => {
    finish(request.id)
    request.resolve()
  }
  return (
    <Sheet
      open={open}
      onClose={settle}
      title={title}
      width={details ? 520 : 420}
      initialFocus={okRef}
      footer={
        <>
          {details ? <CopyButton text={details} label="Copy details" /> : null}
          <Button ref={okRef} variant="prominent" onClick={settle}>
            {okLabel}
          </Button>
        </>
      }
    >
      {message ? <p className={styles.message}>{message}</p> : null}
      {details ? <pre className={`${styles.details} selectable`}>{details}</pre> : null}
    </Sheet>
  )
}
