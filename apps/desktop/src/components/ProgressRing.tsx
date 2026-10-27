import type { CSSProperties } from 'react'
import { useReducedMotion } from './BrailleSpinner'
import styles from './ProgressRing.module.css'

export interface ProgressRingProps {
  /** 0…1, or null while the total is not known. */
  fraction: number | null
  /** Diameter in px; 16 by default. */
  size?: number
  /** Stroke width in px; 2 by default. */
  lineWidth?: number
  /** Any CSS colour; `var(--accent)` by default. */
  tint?: string
  className?: string
  style?: CSSProperties
}

/**
 * `ProgressRing` of the macOS app: circular progress for work with a known
 * size, such as a project opening and building its index (the recent cards,
 * the empty chat's index card): a hairline track and an accent arc filled to
 * `fraction`. Until the total is known a quarter arc turns once a second;
 * with Reduce Motion it stands still. Other async waits keep the spinner.
 */
export function ProgressRing({ fraction, size = 16, lineWidth = 2, tint = 'var(--accent)', className, style }: ProgressRingProps) {
  const reduced = useReducedMotion()
  const centre = size / 2
  // The stroke is centred on the circle: keep all of it inside the box.
  const radius = (size - lineWidth) / 2
  const circumference = 2 * Math.PI * radius
  const length = fraction === null ? 0.25 : Math.min(Math.max(fraction, 0), 1)
  const spinning = fraction === null && !reduced
  const label = fraction === null ? 'Working' : 'Progress'
  return (
    <svg
      role={fraction === null ? 'img' : 'progressbar'}
      aria-label={label}
      aria-valuemin={fraction === null ? undefined : 0}
      aria-valuemax={fraction === null ? undefined : 100}
      aria-valuenow={fraction === null ? undefined : Math.round(length * 100)}
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      className={[styles.ring, spinning ? styles.spinning : '', className ?? ''].filter(Boolean).join(' ')}
      style={style}
    >
      <circle cx={centre} cy={centre} r={radius} fill="none" stroke="var(--border-strong)" strokeWidth={lineWidth} />
      {length > 0 ? (
        <circle
          className={styles.arc}
          cx={centre}
          cy={centre}
          r={radius}
          fill="none"
          stroke={tint}
          strokeWidth={lineWidth}
          strokeLinecap="round"
          strokeDasharray={`${length * circumference} ${circumference}`}
          transform={`rotate(-90 ${centre} ${centre})`}
        />
      ) : null}
    </svg>
  )
}
