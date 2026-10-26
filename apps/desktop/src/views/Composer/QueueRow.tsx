// Port of the queue rows in apps/macos/ContextAgent/Views/Composer/ComposerView.swift:
// `QueueRow` ("Queued · N", prompts waiting for the run to end), `SteeringRow`
// ("Steering · N", prompts sent to the running turn) and their `QueuedItem` chips.

import { memo, useState } from 'react'
import { IconButton } from '../../components/IconButton'
import { Tooltip } from '../../components/Tooltip'
import { useApp, useChatValue } from '../../state/app'
import { ChatState, type OutgoingPrompt } from '../../state/chat'
import { Icon } from '../../theme/icons'
import styles from './QueueRow.module.css'

export interface QueueRowProps {
  chat: ChatState
}

/// `QueueRow` (ComposerView.swift): "Queued · N" with one chip per queued
/// prompt: a click edits it (moves the text back into the composer), ×
/// removes it. Renders nothing while the queue is empty.
export function QueueRow({ chat }: QueueRowProps) {
  const queued = useChatValue(chat, (c) => c.queued, [] as OutgoingPrompt[])
  if (queued.length === 0) return null
  return (
    <div className={styles.row} role="group" aria-label="Queued prompts">
      <span className={styles.label}>Queued · {queued.length}</span>
      <div className={styles.scroller}>
        <div className={styles.items}>
          {queued.map((item, index) => (
            <QueuedItem
              key={index}
              index={index}
              text={item.text}
              attachmentCount={item.images.length + item.files.length}
              chat={chat}
              editable
            />
          ))}
        </div>
      </div>
    </div>
  )
}

/// `SteeringRow` (ComposerView.swift): "Steering · N": prompts sent to the
/// running turn that the model reads at its next step; they cannot be taken
/// back. Renders nothing while there are none.
export function SteeringRow({ chat }: QueueRowProps) {
  const steering = useChatValue(chat, (c) => c.steering, [] as OutgoingPrompt[])
  if (steering.length === 0) return null
  return (
    <Tooltip content="The agent reads these at its next step">
      <div className={styles.row} role="group" aria-label="Messages for the running task">
        <span className={styles.label}>Steering · {steering.length}</span>
        <div className={styles.scroller}>
          <div className={styles.items}>
            {steering.map((item, index) => (
              <QueuedItem
                key={index}
                index={index}
                text={item.text}
                attachmentCount={item.images.length + item.files.length}
                chat={chat}
                editable={false}
              />
            ))}
          </div>
        </div>
      </div>
    </Tooltip>
  )
}

interface QueuedItemProps {
  index: number
  text: string
  attachmentCount: number
  chat: ChatState
  /** A queued prompt (click edits, × removes); a steering one has no actions. */
  editable: boolean
}

/** `QueuedItem`: a queued prompt (edit, remove) or, without actions, a steering one. */
const QueuedItem = memo(function QueuedItem({ index, text, attachmentCount, chat, editable }: QueuedItemProps) {
  const [hovering, setHovering] = useState(false)
  const clipped = ChatState.clip(text, 48)
  const body = (
    <button
      type="button"
      className={[styles.itemButton, editable ? styles.itemButtonEditable : ''].filter(Boolean).join(' ')}
      disabled={!editable}
      aria-label={editable ? `Queued: ${text}. Click to edit` : text}
      onClick={() => {
        if (editable) useApp.getState().editQueued(index, chat)
      }}
    >
      {attachmentCount > 0 ? (
        <span className={styles.attach}>
          <Icon name="attach" size={10} strokeWidth={2} aria-label={`${attachmentCount} attachment${attachmentCount === 1 ? '' : 's'}`} />
        </span>
      ) : null}
      <span className={styles.itemText}>{clipped}</span>
    </button>
  )
  return (
    <div
      className={[styles.item, editable ? styles.itemEditable : '', hovering && editable ? styles.itemHover : ''].filter(Boolean).join(' ')}
      onPointerEnter={() => setHovering(true)}
      onPointerLeave={() => setHovering(false)}
    >
      {editable ? <Tooltip content="Click to edit">{body}</Tooltip> : body}
      {editable ? (
        <IconButton icon="close" label="Remove from queue" size={9} onClick={() => useApp.getState().removeQueued(index, chat)} />
      ) : null}
    </div>
  )
})
