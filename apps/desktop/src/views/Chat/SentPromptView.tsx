// Port of apps/macos/ContextAgent/Views/Chat/SentPromptView.swift.

import { useState } from 'react'
import { sentPromptEstimatedTokens, sentPromptPlainText, type SentPrompt, type SentPromptMessage } from '../../backend/types'
import { CopyButton } from '../../components/CopyButton'
import { DisclosureRow } from '../../components/DisclosureRow'
import { TranscriptFormat } from '../../models/transcript'
import { Icon } from '../../theme/icons'
import styles from './SentPromptView.module.css'

export interface SentPromptViewProps {
  prompt: SentPrompt
}

/**
 * `SentPromptView` (SentPromptView.swift): developer mode's collapsed "Full
 * prompt" row above the message that started a run. Open, it lists every
 * message of the run's first model request (each its own collapsed row, so a
 * long repository context is only laid out when asked for) and the tools
 * offered, with Copy.
 */
export function SentPromptView({ prompt }: SentPromptViewProps) {
  const [expanded, setExpanded] = useState(false)
  const summary = promptSummary(prompt)
  return (
    <div className={styles.root}>
      <DisclosureRow
        expanded={expanded}
        onToggle={setExpanded}
        aria-label={`Full prompt, ${summary}`}
        label={
          <>
            <Icon name="prompt" size={12} strokeWidth={2} className={styles.icon} />
            <span className={styles.title}>Full prompt</span>
            <span className={styles.caption}>{summary}</span>
          </>
        }
      >
        <div className={styles.content}>
          {prompt.messages.map((message, index) => (
            <SentPromptMessageRow key={index} message={message} />
          ))}
          {prompt.tools.length > 0 ? <div className={`${styles.tools} selectable`}>{'Tools: ' + prompt.tools.join(', ')}</div> : null}
          <div className={styles.copyRow}>
            <CopyButton text={sentPromptPlainText(prompt)} label="Copy the full prompt" />
          </div>
        </div>
      </DisclosureRow>
    </div>
  )
}

/** "4 messages · ~15.3k tok · 60 tools". */
function promptSummary(prompt: SentPrompt): string {
  const count = prompt.messages.length
  const parts = [`${count} message${count === 1 ? '' : 's'}`, `~${TranscriptFormat.tokens(sentPromptEstimatedTokens(prompt))} tok`]
  if (prompt.tools.length > 0) parts.push(`${prompt.tools.length} tool${prompt.tools.length === 1 ? '' : 's'}`)
  return parts.join(' · ')
}

/** `SentPromptMessageRow`: one message of the request, collapsed until asked for. */
function SentPromptMessageRow({ message }: { message: SentPromptMessage }) {
  const [expanded, setExpanded] = useState(false)
  const title = message.kind.replace(/_/g, ' ')
  const parts = [`~${TranscriptFormat.tokens(message.estimated_tokens)} tok`]
  if (message.images > 0) parts.push(`${message.images} image${message.images === 1 ? '' : 's'}`)
  return (
    <DisclosureRow
      expanded={expanded}
      onToggle={setExpanded}
      label={
        <>
          <span className={styles.messageTitle}>{title}</span>
          <span className={styles.caption}>{parts.join(' · ')}</span>
        </>
      }
    >
      {/* One monospace block (a `pre`), selectable. */}
      <pre className={`${styles.messageText} selectable`}>{message.text}</pre>
    </DisclosureRow>
  )
}
