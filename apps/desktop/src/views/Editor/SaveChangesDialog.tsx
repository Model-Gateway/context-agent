import { useRef, useSyncExternalStore } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { Button } from '../../components/Button'
import { Sheet } from '../../components/Sheet'
import styles from './SaveChangesDialog.module.css'

/**
 * The `NSAlert` of `closeDocument(_:in:)` / `closeDocuments(_:selecting:in:)`
 * (App/EditorState.swift): "Save changes to … ?" with Save · Don't Save ·
 * Cancel, as a promise. `confirm()` in components/dialogs.tsx has two
 * buttons only, so this is the same queue-and-root pattern with three; it
 * renders in a root of its own under `document.body`, so state/editor.ts can
 * ask without a host.
 */

/** What the user chose: the first, second or third button (Esc cancels). */
export type SaveChoice = 'save' | 'dontSave' | 'cancel'

export interface SaveChangesOptions {
  /** `messageText`: "Save changes to main.rs?". */
  title: string
  /** `informativeText`. */
  message: string
  /** "Save", or "Save All" for several files. */
  saveLabel: string
}

interface Request {
  id: number
  options: SaveChangesOptions
  resolve: (choice: SaveChoice) => void
}

let queue: Request[] = []
let nextId = 1
const listeners = new Set<() => void>()
let root: Root | null = null

function emit(): void {
  for (const listener of [...listeners]) listener()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function snapshot(): Request[] {
  return queue
}

function ensureRoot(): void {
  if (root || typeof document === 'undefined') return
  const container = document.createElement('div')
  container.setAttribute('data-ctx-save-dialog', '')
  document.body.appendChild(container)
  root = createRoot(container)
  root.render(<SaveDialogHost />)
}

/** `alert.runModal()` with Save · Don't Save · Cancel; resolves with the choice. */
export function askToSaveChanges(options: SaveChangesOptions): Promise<SaveChoice> {
  return new Promise((resolve) => {
    queue = [...queue, { id: nextId++, options, resolve }]
    ensureRoot()
    emit()
  })
}

function SaveDialogHost() {
  const requests = useSyncExternalStore(subscribe, snapshot, snapshot)
  const current = requests[0]
  if (!current) return null
  return <SaveChangesSheet key={current.id} request={current} />
}

function SaveChangesSheet({ request }: { request: Request }) {
  const saveRef = useRef<HTMLButtonElement>(null)
  const { title, message, saveLabel } = request.options
  const settle = (choice: SaveChoice) => {
    queue = queue.filter((candidate) => candidate.id !== request.id)
    emit()
    request.resolve(choice)
  }
  return (
    <Sheet
      open
      onClose={() => settle('cancel')}
      title={title}
      width={420}
      initialFocus={saveRef}
      dismissOnBackdrop={false}
      footer={
        <>
          <Button variant="bordered" className={styles.dontSave} onClick={() => settle('dontSave')}>
            Don&apos;t Save
          </Button>
          <Button variant="bordered" onClick={() => settle('cancel')}>
            Cancel
          </Button>
          <Button ref={saveRef} variant="prominent" onClick={() => settle('save')}>
            {saveLabel}
          </Button>
        </>
      }
    >
      <p className={styles.message}>{message}</p>
    </Sheet>
  )
}
