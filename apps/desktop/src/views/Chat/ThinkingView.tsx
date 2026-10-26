// Port of apps/macos/ContextAgent/Views/Chat/ThinkingView.swift.

import { useEffect, useRef, useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { DisclosureRow } from '../../components/DisclosureRow'
import { Tooltip } from '../../components/Tooltip'
import { TranscriptFormat, type AssistantTurn } from '../../models/transcript'
import { Icon } from '../../theme/icons'
import styles from './ThinkingView.module.css'

export interface ThinkingViewProps {
  turn: AssistantTurn
}

/**
 * `ThinkingView` (ThinkingView.swift): the collapsible reasoning row above an
 * assistant message: "Thinking ⠋ 1.2s" while reasoning streams, "Thinking ·
 * 1.2s" once the answer has started. It is open while the model thinks and
 * collapses on the first text delta; the user can reopen it at any time,
 * and a click anywhere in the open reasoning closes it again.
 */
export function ThinkingView({ turn }: ThinkingViewProps) {
  const [expanded, setExpanded] = useState(turn.isThinking)
  const wasThinking = useRef(turn.isThinking)

  // Auto-collapse once the answer starts; a manual reopen stays open.
  useEffect(() => {
    if (wasThinking.current && !turn.isThinking) setExpanded(false)
    wasThinking.current = turn.isThinking
  }, [turn.isThinking])

  /// A click anywhere in the reasoning collapses it, so a long one can be
  /// closed without scrolling back up to the header. A drag that selected
  /// text is not a click (the Swift tap gesture runs alongside selection).
  const collapseOnClick = () => {
    const selection = window.getSelection()
    if (selection && !selection.isCollapsed && selection.toString().length > 0) return
    setExpanded(false)
  }

  let accessibilityText = 'Thinking'
  if (!turn.isThinking && turn.thinkingDuration !== null) accessibilityText = `Thought for ${thinkingElapsedLabel(turn.thinkingDuration)}`

  return (
    <DisclosureRow
      expanded={expanded}
      onToggle={setExpanded}
      aria-label={accessibilityText}
      label={
        <>
          <Icon name="thinking" size={12} strokeWidth={2} className={styles.icon} />
          <span className={styles.title}>Thinking</span>
          {turn.isThinking ? (
            <>
              <BrailleSpinner />
              {turn.thinkingStarted !== null ? <LiveElapsed started={turn.thinkingStarted} /> : null}
            </>
          ) : turn.thinkingDuration !== null ? (
            <span className={styles.elapsed}>{thinkingElapsedLabel(turn.thinkingDuration)}</span>
          ) : null}
        </>
      }
    >
      {/* One text block holding the whole reasoning is re-laid out on
          every 33 ms flush; while streaming only the tail is shown, once
          finished each paragraph is its own block. */}
      <Tooltip content="Click to collapse">
        <div className={styles.content} onClick={collapseOnClick}>
          {turn.isThinking ? (
            <p className={`${styles.reasoning} selectable`}>{thinkingStreamingTail(turn.reasoning)}</p>
          ) : (
            thinkingParagraphs(turn.reasoning).map((paragraph, index) => (
              <p key={index} className={`${styles.reasoning} selectable`}>
                {paragraph}
              </p>
            ))
          )}
        </div>
      </Tooltip>
    </DisclosureRow>
  )
}

/** `TimelineView(.periodic(from: .now, by: 0.1))` over the elapsed thinking time. */
function LiveElapsed({ started }: { started: Date }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 100)
    return () => window.clearInterval(timer)
  }, [])
  return <span className={styles.elapsed}>{thinkingElapsedLabel((now - started.getTime()) / 1000)}</span>
}

/** `ThinkingView.streamingTailLength`: characters of live reasoning shown while it streams. */
export const THINKING_STREAMING_TAIL_LENGTH = 1_500

/// `ThinkingView.streamingTail(_:)`: the last `streamingTailLength`
/// characters, starting at a line (else word) boundary and led by "…" when
/// anything was cut.
export function thinkingStreamingTail(reasoning: string): string {
  if (utf8Count(reasoning) <= THINKING_STREAMING_TAIL_LENGTH) return reasoning
  // At most two UTF-16 units per character: slice first so a long reasoning
  // is not split into characters on every flush.
  let tail = Array.from(reasoning.slice(-THINKING_STREAMING_TAIL_LENGTH * 2)).slice(-THINKING_STREAMING_TAIL_LENGTH).join('')
  const newline = tail.indexOf('\n')
  if (newline >= 0) {
    tail = tail.slice(newline + 1)
  } else {
    const space = tail.indexOf(' ')
    if (space >= 0) tail = tail.slice(space + 1)
  }
  return '…' + tail
}

/** `ThinkingView.paragraphs(_:)`: blank-line separated paragraphs, trimmed, empties dropped. */
export function thinkingParagraphs(reasoning: string): string[] {
  return reasoning
    .split('\n\n')
    .map((paragraph) => paragraph.trim())
    .filter((paragraph) => paragraph.length > 0)
}

/** `ThinkingView.elapsedLabel(_:)`: 1.234 → "1.2s"; ≥ 60 s falls back to the shared "1m 05s" style. */
export function thinkingElapsedLabel(seconds: number): string {
  const clamped = Math.max(0, seconds)
  if (clamped < 60) return `${clamped.toFixed(1)}s`
  return TranscriptFormat.duration(clamped)
}

/** `String.utf8.count` without allocating (the reasoning can run to tens of KB). */
function utf8Count(text: string): number {
  // Every UTF-16 unit is at least one UTF-8 byte: short strings need no count.
  if (text.length > THINKING_STREAMING_TAIL_LENGTH) return text.length
  let bytes = 0
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i)
    if (code < 0x80) bytes += 1
    else if (code < 0x800) bytes += 2
    else if (code >= 0xd800 && code <= 0xdbff) {
      bytes += 4
      i += 1
    } else bytes += 3
  }
  return bytes
}
