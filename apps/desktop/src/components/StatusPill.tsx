import type { CSSProperties } from 'react'
import { BrailleSpinner } from './BrailleSpinner'
import styles from './StatusPill.module.css'

/**
 * `TaskStatus` of Protocol.swift (lowercase wire values). Structurally the
 * same union as `TaskStatus` in src/backend/types.ts.
 */
export type StatusPillStatus = 'completed' | 'blocked' | 'failed' | 'cancelled'

interface CommonProps {
  className?: string
  style?: CSSProperties
}

export type StatusPillProps = CommonProps &
  (
    | { /** `StatusPill(_ status:)` */ status: StatusPillStatus; running?: never; text?: never; tint?: never }
    | { /** `StatusPill(running:)`: Running (with a spinner) / Idle */ running: boolean; status?: never; text?: never; tint?: never }
    | { /** `StatusPill(text:tint:)` */ text: string; tint: string; status?: never; running?: never }
  )

const STATUS_LABELS: Record<StatusPillStatus, { text: string; tint: string }> = {
  completed: { text: 'Completed', tint: 'var(--success)' },
  blocked: { text: 'Blocked', tint: 'var(--warning)' },
  failed: { text: 'Failed', tint: 'var(--danger)' },
  cancelled: { text: 'Cancelled', tint: 'var(--text-secondary)' },
}

/**
 * `StatusPill` of the macOS app: a tinted capsule that always carries the
 * status word in text, never colour alone: Completed / Blocked / Failed /
 * Cancelled / Running / Idle.
 */
export function StatusPill(props: StatusPillProps) {
  let text: string
  let tint: string
  let isRunning = false
  if (props.status !== undefined) {
    ;({ text, tint } = STATUS_LABELS[props.status])
  } else if (props.running !== undefined) {
    text = props.running ? 'Running' : 'Idle'
    tint = props.running ? 'var(--accent)' : 'var(--text-tertiary)'
    isRunning = props.running
  } else {
    text = props.text
    tint = props.tint
  }
  return (
    <span className={[styles.pill, props.className ?? ''].filter(Boolean).join(' ')} style={{ ...props.style, ['--tint' as string]: tint }}>
      {isRunning ? <BrailleSpinner size={10} tint="currentColor" /> : <span className={styles.dot} aria-hidden="true" />}
      <span className={styles.text}>{text}</span>
    </span>
  )
}
