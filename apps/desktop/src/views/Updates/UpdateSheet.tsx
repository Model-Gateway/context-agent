// What Sparkle's standard windows show in the macOS app (App/Updater.swift),
// as one sheet over the main window: "Checking for updates…", "You're up to
// date!", the new release with its notes (Skip This Version, Remind Me
// Later, Install Update), the download, "Ready to Install" (Install on Quit,
// Install and Relaunch) and, when a task runs, Updater.postponeRelaunch's
// question. The state lives in state/updater.ts.

import { useRef } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Sheet } from '../../components/Sheet'
import { MarkdownView } from '../../markdown/MarkdownView'
import {
  closeUpdateSheet,
  dismissUpdate,
  downloadUpdate,
  installAndRelaunch,
  installWhenTasksFinish,
  useUpdater,
  type UpdatePhase,
  type UpdateSheetContent,
  type UpdaterStatus,
} from '../../state/updater'
import styles from './UpdateSheet.module.css'

/** Mounted once in MainWindow; open while `useUpdater().sheet` is set. */
export function UpdateSheet() {
  const sheet = useUpdater((state) => state.sheet)
  const status = useUpdater((state) => state.status)
  const primaryRef = useRef<HTMLButtonElement>(null)
  // Drawn from the last content while the sheet slides away.
  const last = useRef<{ sheet: UpdateSheetContent; status: UpdaterStatus } | null>(null)
  if (sheet && status) last.current = { sheet, status }
  const shown = last.current
  if (!shown) return null
  const content = describe(shown.sheet, shown.status)
  return (
    <Sheet
      open={sheet !== null && status !== null}
      onClose={content.onClose ?? closeUpdateSheet}
      title={content.title}
      width={480}
      initialFocus={primaryRef}
      footer={content.buttons.map((button, index) => (
        <Button
          key={button.label}
          ref={index === content.buttons.length - 1 ? primaryRef : undefined}
          variant={index === content.buttons.length - 1 && content.buttons.length > 1 ? 'prominent' : 'bordered'}
          onClick={button.action}
        >
          {button.label}
        </Button>
      ))}
    >
      <div className={styles.body}>
        {content.message ? <p className={styles.message}>{content.message}</p> : null}
        {content.progress ? (
          <div className={styles.progress} role="status">
            <BrailleSpinner size={13} />
            <span>{content.progress}</span>
          </div>
        ) : null}
        {content.notes ? (
          <div className={styles.notes} aria-label="Release notes">
            <MarkdownView markdown={content.notes} />
          </div>
        ) : null}
      </div>
    </Sheet>
  )
}

interface SheetButton {
  label: string
  action: () => void
}

interface SheetDescription {
  title: string
  message?: string
  progress?: string
  notes?: string | null
  /** In footer order; the last is the default (prominent) button. */
  buttons: SheetButton[]
  /** Esc and the backdrop; closes the sheet by default. */
  onClose?: () => void
}

function describe(sheet: UpdateSheetContent, status: UpdaterStatus): SheetDescription {
  switch (sheet.kind) {
    case 'checking':
      return {
        title: 'Checking for updates…',
        progress: 'Looking for a new release on GitHub',
        buttons: [{ label: 'Cancel', action: closeUpdateSheet }],
      }
    case 'upToDate':
      return {
        title: 'You’re up to date!',
        message: `Context ${status.version} is currently the newest version available.`,
        buttons: [{ label: 'OK', action: closeUpdateSheet }],
      }
    case 'postpone':
      return {
        title: 'Restart Context to finish updating?',
        message: 'A task is still running, and restarting now stops it. Context can restart by itself as soon as every running task has finished.',
        buttons: [
          { label: 'Cancel', action: closeUpdateSheet },
          { label: 'Restart Now', action: () => void installAndRelaunch(true) },
          { label: 'Restart When Done', action: installWhenTasksFinish },
        ],
      }
    case 'release':
      return describeRelease(status.phase, status.version)
  }
}

function describeRelease(phase: UpdatePhase, current: string): SheetDescription {
  switch (phase.kind) {
    case 'available':
      return {
        title: 'A new version of Context is available!',
        message: `Context ${phase.release.version} is now available—you have ${current}. Would you like to download it now?`,
        notes: phase.release.notes,
        buttons: [
          { label: 'Skip This Version', action: () => void dismissUpdate(true) },
          { label: 'Remind Me Later', action: () => void dismissUpdate(false) },
          { label: 'Install Update', action: () => void downloadUpdate() },
        ],
        onClose: () => void dismissUpdate(false),
      }
    case 'downloading':
      return {
        title: 'Downloading update…',
        message: `Context ${phase.release.version} installs when the download is done and you restart or quit Context.`,
        progress: downloadProgress(phase.received, phase.total),
        buttons: [{ label: 'Hide', action: closeUpdateSheet }],
      }
    case 'ready':
      return {
        title: 'Ready to Install',
        message: `Context ${phase.release.version} has been downloaded and is ready to use. Restart Context now to finish updating, or it installs when you quit.`,
        notes: phase.release.notes,
        buttons: [
          { label: 'Install on Quit', action: closeUpdateSheet },
          { label: 'Install and Relaunch', action: () => void installAndRelaunch() },
        ],
      }
    case 'idle':
    case 'checking':
      // setStatus closes the sheet in these phases; this is its last frame.
      return { title: 'Checking for updates…', buttons: [{ label: 'Cancel', action: closeUpdateSheet }] }
  }
}

/** "12.3 MB of 20.4 MB" (Sparkle's download window). */
function downloadProgress(received: number, total: number | null): string {
  const mb = (bytes: number) => `${(bytes / 1_000_000).toFixed(1)} MB`
  return total !== null && total > 0 ? `${mb(received)} of ${mb(total)}` : mb(received)
}
