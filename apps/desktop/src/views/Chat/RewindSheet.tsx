// Port of apps/macos/ContextAgent/Views/Chat/RewindSheet.swift (the sheet;
// `RewindRequest` itself lives in state/app.ts) and of ChatView's
// `.sheet(item: $appState.rewindRequest)` that presents it.

import { useRef, type KeyboardEvent, type RefObject } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Sheet } from '../../components/Sheet'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { useApp, type RewindRequest } from '../../state/app'
import { ChatState } from '../../state/chat'
import { Icon, type IconName } from '../../theme/icons'
import styles from './RewindSheet.module.css'

/** `RewindSheet.maxListedFiles`. */
const MAX_LISTED_FILES = 8

/**
 * `RewindSheet` of the macOS app: the keep-or-revert modal for an edit of a
 * sent message or an undo of a turn (`rewindRequest` in the store; renders
 * nothing while it is null). It says what the rewind removes and which
 * files changed since, then offers Cancel (Esc), *Keep files* and *Revert
 * files* (the default, Return, when a snapshot exists; without one only
 * *Keep files*, and the hint turns `warning`).
 */
export function RewindSheet() {
  const request = useApp((state) => state.rewindRequest)
  const inFlight = useApp((state) => state.rewindInFlight)

  // No button takes the focus (a macOS sheet shows none focused); Return
  // still reaches the default button through the content's handler.
  const contentRef = useRef<HTMLDivElement>(null)

  const cancel = () => {
    if (useApp.getState().rewindInFlight) return
    useApp.getState().setRewindRequest(null)
  }

  return (
    <Sheet
      open={request !== null}
      onClose={cancel}
      width={460}
      dismissOnBackdrop={false}
      initialFocus={contentRef}
      aria-label={request ? title(request) : undefined}
    >
      {request ? <RewindContent request={request} inFlight={inFlight} onCancel={cancel} contentRef={contentRef} /> : null}
    </Sheet>
  )
}

function RewindContent({
  request,
  inFlight,
  onCancel,
  contentRef,
}: {
  request: RewindRequest
  inFlight: boolean
  onCancel: () => void
  contentRef: RefObject<HTMLDivElement | null>
}) {
  const hasSnapshot = request.turn.hasSnapshot

  const perform = (restoreFiles: boolean) => {
    if (useApp.getState().rewindInFlight) return
    void useApp.getState().performRewind(request, restoreFiles)
  }

  // Return is the default button (Revert files with a snapshot, else Keep
  // files) wherever the focus is, like `.keyboardShortcut(.defaultAction)`.
  // Capture phase: the sheet's focus trap stops Enter from bubbling.
  const onKeyDownCapture = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Enter' || event.nativeEvent.isComposing || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return
    event.preventDefault()
    if (inFlight) return
    perform(hasSnapshot)
  }

  return (
    <div ref={contentRef} tabIndex={-1} className={styles.content} onKeyDownCapture={onKeyDownCapture}>
      <div className={styles.top}>
        <span className={styles.topIcon} aria-hidden="true">
          <Icon name="undo" size={22} strokeWidth={2} />
        </span>
        <div className={styles.titles}>
          <span className={styles.title}>{title(request)}</span>
          <span className={styles.subtitle}>{subtitle(request)}</span>
        </div>
      </div>

      <div className={styles.box}>
        <LabelRow icon="chat" text={messagesLine(request.laterMessages)} />
        <LabelRow icon="file" text={filesLine(request.changedFiles.length)} />
        {request.changedFiles.length > 0 ? (
          <div className={styles.files}>
            {request.changedFiles.slice(0, MAX_LISTED_FILES).map((path) => (
              <MiddleTruncate key={path} text={path} className={styles.path} />
            ))}
            {request.changedFiles.length > MAX_LISTED_FILES ? <span className={styles.more}>+ {request.changedFiles.length - MAX_LISTED_FILES} more</span> : null}
          </div>
        ) : null}
      </div>

      <p className={styles.hint} style={{ color: hasSnapshot ? 'var(--text-tertiary)' : 'var(--warning)' }}>
        {choiceHint(hasSnapshot)}
      </p>

      <div className={styles.buttons}>
        <Button variant="bordered" disabled={inFlight} onClick={onCancel}>
          Cancel
        </Button>
        {inFlight ? <BrailleSpinner size={13} tint="var(--text-secondary)" className={styles.spinner} /> : null}
        <span className={styles.spacer} />
        {hasSnapshot ? (
          <>
            <Button variant="bordered" disabled={inFlight} onClick={() => perform(false)}>
              Keep files
            </Button>
            <Button variant="prominent" disabled={inFlight} onClick={() => perform(true)}>
              Revert files
            </Button>
          </>
        ) : (
          <Button variant="prominent" disabled={inFlight} onClick={() => perform(false)}>
            Keep files
          </Button>
        )}
      </div>
    </div>
  )
}

/** SwiftUI `Label(text, systemImage:)` at 13 px. */
function LabelRow({ icon, text }: { icon: IconName; text: string }) {
  return (
    <div className={styles.label}>
      <span className={styles.labelIcon} aria-hidden="true">
        <Icon name={icon} size={12.5} strokeWidth={1.75} />
      </span>
      <span>{text}</span>
    </div>
  )
}

function title(request: RewindRequest): string {
  return request.mode.kind === 'edit' ? 'Edit this message?' : 'Undo this turn?'
}

function subtitle(request: RewindRequest): string {
  const quoted = ChatState.clip(request.turn.text, 72)
  return request.mode.kind === 'edit'
    ? `The chat goes back to before “${quoted}” and the edited message is sent in its place.`
    : `The chat goes back to before “${quoted}”; the message returns to the composer.`
}

function messagesLine(laterMessages: number): string {
  switch (laterMessages) {
    case 0:
      return 'This message and its reply are removed.'
    case 1:
      return 'This message, its reply and 1 later message are removed.'
    default:
      return `This message, its reply and ${laterMessages} later messages are removed.`
  }
}

function filesLine(count: number): string {
  switch (count) {
    case 0:
      return 'No file changes were recorded after this message.'
    case 1:
      return '1 file changed since then:'
    default:
      return `${count} files changed since then:`
  }
}

function choiceHint(hasSnapshot: boolean): string {
  if (hasSnapshot) {
    return 'Revert puts the project files back to how they were before this message (ignored files and commits are untouched). Keep leaves them as they are now.'
  }
  return 'No snapshot was taken before this message (the project had no Git repository), so the files can only be kept.'
}
