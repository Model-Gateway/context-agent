import type { ReactNode } from 'react'
import styles from './InspectorCard.module.css'

export interface InspectorCardProps {
  /** Shown uppercased, 11 px semibold, tertiary. */
  title: string
  children?: ReactNode
}

/**
 * `InspectorCard` (Views/Inspector/InspectorView.swift): the Run tab's
 * section card — an uppercased title over its rows 10 px apart, 12 px
 * padding, `surface` with a hairline border and the 12 px radius.
 */
export function InspectorCard({ title, children }: InspectorCardProps) {
  return (
    <section className={styles.card} aria-label={title}>
      <h3 className={styles.title}>{title}</h3>
      {children}
    </section>
  )
}

export interface StatRowProps {
  label: string
  value: string
  /** Any CSS colour; `var(--text-primary)` by default. */
  tint?: string
}

/**
 * `StatRow` (InspectorView.swift): "label ····· value", the value in 12 px
 * medium monospace on one line (truncated at the tail) and selectable.
 */
export function StatRow({ label, value, tint }: StatRowProps) {
  return (
    <div className={styles.statRow}>
      <span className={styles.statLabel}>{label}</span>
      <span className={styles.statSpacer} />
      <span className={`${styles.statValue} selectable`} style={tint ? { color: tint } : undefined}>
        {value}
      </span>
    </div>
  )
}
