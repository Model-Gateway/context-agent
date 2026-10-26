import { useRef, type KeyboardEvent } from 'react'
import styles from './SegmentedControl.module.css'

export interface SegmentedControlProps<T extends string> {
  options: ReadonlyArray<{ value: T; label: string }>
  value: T
  onChange: (value: T) => void
  'aria-label': string
  /** `.frame(maxWidth:)`: stretches to the container up to this width (`Infinity`: no cap); the segments share it equally. Without it the control hugs its labels (`.fixedSize()`). */
  maxWidth?: number
  disabled?: boolean
  className?: string
}

/**
 * `Picker(…).pickerStyle(.segmented).labelsHidden()` of the macOS views: an
 * NSSegmentedControl as macOS 26 draws it (a native render): a grey 24 px
 * track of equally wide segments, the selected one filled with the accent,
 * hairlines between the others. Left / Right / Home / End move the
 * selection. Settings (Appearance, Spending) and the Account usage card use
 * this one control.
 */
export function SegmentedControl<T extends string>({ options, value, onChange, maxWidth, disabled = false, className, ...aria }: SegmentedControlProps<T>) {
  const listRef = useRef<HTMLDivElement>(null)

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (disabled || options.length === 0) return
    const current = Math.max(
      0,
      options.findIndex((option) => option.value === value),
    )
    let next: number
    switch (event.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        next = Math.min(options.length - 1, current + 1)
        break
      case 'ArrowLeft':
      case 'ArrowUp':
        next = Math.max(0, current - 1)
        break
      case 'Home':
        next = 0
        break
      case 'End':
        next = options.length - 1
        break
      default:
        return
    }
    event.preventDefault()
    const option = options[next]
    if (!option) return
    if (option.value !== value) onChange(option.value)
    listRef.current?.querySelectorAll<HTMLButtonElement>('button')[next]?.focus()
  }

  return (
    <div
      ref={listRef}
      role="radiogroup"
      aria-label={aria['aria-label']}
      aria-disabled={disabled || undefined}
      className={[styles.picker, maxWidth !== undefined ? styles.stretch : '', disabled ? styles.disabled : '', className ?? ''].filter(Boolean).join(' ')}
      style={maxWidth !== undefined && Number.isFinite(maxWidth) ? { maxWidth } : undefined}
      onKeyDown={onKeyDown}
    >
      {options.map((option) => {
        const selected = option.value === value
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            disabled={disabled}
            className={selected ? `${styles.segment} ${styles.selected}` : styles.segment}
            onClick={() => {
              if (!selected) onChange(option.value)
            }}
          >
            <span className={styles.label}>{option.label}</span>
          </button>
        )
      })}
    </div>
  )
}
