import type { CSSProperties } from 'react'
import { useReducedMotion } from './BrailleSpinner'
import styles from './InlineDots.module.css'

export interface InlineDotsProps {
  /** Any CSS colour; `var(--text-tertiary)` by default. */
  tint?: string
  className?: string
  style?: CSSProperties
}

/**
 * `InlineDots` of the macOS app: three 4 px dots for inline "still going"
 * hints (queue rows, streaming tails); one lights up every 0.3 s, all three
 * stay lit with Reduce Motion.
 */
export function InlineDots({ tint = 'var(--text-tertiary)', className, style }: InlineDotsProps) {
  const reduced = useReducedMotion()
  return (
    <span
      role="img"
      aria-label="Working"
      className={[styles.dots, reduced ? styles.static : '', className ?? ''].filter(Boolean).join(' ')}
      style={{ color: tint, ...style }}
    >
      <span className={styles.dot} />
      <span className={styles.dot} />
      <span className={styles.dot} />
    </span>
  )
}
