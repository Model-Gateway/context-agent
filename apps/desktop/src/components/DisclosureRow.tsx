import { useId, useRef, type MouseEvent, type ReactNode } from 'react'
import { Icon } from '../theme/icons'
import { Fade } from './Fade'
import styles from './DisclosureRow.module.css'

export interface DisclosureRowProps {
  expanded: boolean
  onToggle: (expanded: boolean) => void
  /** The row's content before the chevron (icon, title, spinner). */
  label: ReactNode
  /** Pushed to the row's right edge (elapsed time, a copy button); clicks on it do not toggle. */
  trailing?: ReactNode
  /** Revealed under the row while expanded. */
  children?: ReactNode
  /** Accessible name when the label is not plain text. */
  'aria-label'?: string
  className?: string
  /** Class for the content area (indentation). */
  contentClassName?: string
}

/**
 * `DisclosureRow` of the macOS app: a clickable row with a chevron that
 * rotates when expanded, revealing `children` underneath (the space opens
 * and closes while they fade over 0.18 s, instantly with Reduce Motion). Used by the tool
 * timeline header, Thinking rows and noisy notices. The chevron sits right
 * after the label; `trailing` goes to the right edge (the Swift view's
 * `Spacer()` inside the label). The whole row highlights on hover.
 */
export function DisclosureRow({ expanded, onToggle, label, trailing, children, className, contentClassName, ...aria }: DisclosureRowProps) {
  const contentId = useId()
  const trailingRef = useRef<HTMLDivElement>(null)

  // The row is the hit area (like the Swift row's content shape); the
  // button inside carries focus and the expanded state.
  const onRowClick = (event: MouseEvent<HTMLDivElement>) => {
    if (trailingRef.current?.contains(event.target as Node)) return
    onToggle(!expanded)
  }

  return (
    <div className={[styles.row, className ?? ''].filter(Boolean).join(' ')}>
      <div className={styles.header} onClick={onRowClick}>
        <button
          type="button"
          className={styles.button}
          aria-expanded={expanded}
          aria-controls={expanded ? contentId : undefined}
          aria-label={aria['aria-label']}
        >
          <span className={styles.label}>{label}</span>
          <Icon name="chevronRight" size={11} strokeWidth={2.5} className={[styles.chevron, expanded ? styles.open : ''].filter(Boolean).join(' ')} />
        </button>
        {trailing ? (
          <div ref={trailingRef} className={styles.trailing}>
            {trailing}
          </div>
        ) : null}
      </div>
      {/* Opens and closes with the content (`.transition(.opacity)` in
          `withAnimation(.easeInOut(duration: 0.18))`). */}
      <Fade show={expanded}>
        <div id={contentId} className={[styles.content, contentClassName ?? ''].filter(Boolean).join(' ')}>
          {children}
        </div>
      </Fade>
    </div>
  )
}
