// The day separator of apps/macos/ContextAgent/Views/Chat/ChatView.swift
// (`DaySeparator`, a private struct there).

import styles from './DaySeparator.module.css'

export interface DaySeparatorProps {
  /** "Today", "Yesterday", "Oct 12" (`TranscriptTime.dayLabel`). */
  label: string
}

/**
 * `DaySeparator` (ChatView.swift): a centred day label between two
 * hairlines, before the first item of a calendar day.
 */
export function DaySeparator({ label }: DaySeparatorProps) {
  return (
    <div className={styles.separator} role="separator" aria-label={label}>
      <span className={styles.line} aria-hidden="true" />
      <span className={styles.label} aria-hidden="true">
        {label}
      </span>
      <span className={styles.line} aria-hidden="true" />
    </div>
  )
}
