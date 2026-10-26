import { useRef, type KeyboardEvent } from 'react'
import { Icon, type IconName } from '../theme/icons'
import { Tooltip } from './Tooltip'
import styles from './Segmented.module.css'

export interface SegmentedOption<T extends string> {
  value: T
  label: string
  icon?: IconName
  /** A count capsule after the label (dirty files, unsaved documents). */
  badge?: number
  /** Shown in the tooltip, spelled for the platform ("mod+1"). */
  shortcut?: string
  disabled?: boolean
}

export interface SegmentedProps<T extends string> {
  options: ReadonlyArray<SegmentedOption<T>>
  value: T
  onChange: (value: T) => void
  /** Shows the title on the selected pill only (narrow top bars). */
  compact?: boolean
  disabled?: boolean
  /** 28 px pills (default) or 24 px ones for card headers (Usage: 24h · 7d · 30d · All). */
  size?: 'regular' | 'small'
  /** Glyph size and stroke of the option icons (default 12 px, 2.25). */
  iconSize?: number
  iconStrokeWidth?: number
  'aria-label': string
  className?: string
}

/**
 * `TabSwitcher` / `TabPill` of the macOS DetailView and the segmented
 * pickers in card headers: pill tabs where the selected one is `surface`
 * with a hairline and a soft shadow, the others plain with a hover
 * highlight. Left / Right / Home / End move the selection.
 */
export function Segmented<T extends string>({ options, value, onChange, compact = false, disabled = false, size = 'regular', iconSize = 12, iconStrokeWidth = 2.25, className, ...aria }: SegmentedProps<T>) {
  const listRef = useRef<HTMLDivElement>(null)

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const enabled = options.filter((option) => !option.disabled)
    if (enabled.length === 0) return
    const current = enabled.findIndex((option) => option.value === value)
    let next = -1
    switch (event.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        next = (current + 1) % enabled.length
        break
      case 'ArrowLeft':
      case 'ArrowUp':
        next = (current - 1 + enabled.length) % enabled.length
        break
      case 'Home':
        next = 0
        break
      case 'End':
        next = enabled.length - 1
        break
      default:
        return
    }
    event.preventDefault()
    const option = enabled[next]
    if (!option) return
    onChange(option.value)
    const tab = listRef.current?.querySelector<HTMLButtonElement>(`[data-value="${CSS.escape(option.value)}"]`)
    tab?.focus()
  }

  return (
    <div
      ref={listRef}
      role="tablist"
      aria-label={aria['aria-label']}
      aria-disabled={disabled || undefined}
      className={[styles.list, size === 'small' ? styles.small : '', disabled ? styles.disabled : '', className ?? ''].filter(Boolean).join(' ')}
      onKeyDown={onKeyDown}
    >
      {options.map((option) => {
        const selected = option.value === value
        // Roving tab stop: the selected pill, or the first enabled one when none is selected.
        const tabStop = selected || (!options.some((candidate) => candidate.value === value) && option === options.find((candidate) => !candidate.disabled))
        const showTitle = !compact || selected
        const pill = (
          <button
            key={option.value}
            type="button"
            role="tab"
            data-value={option.value}
            aria-selected={selected}
            aria-label={option.badge ? `${option.label}, ${option.badge}` : option.label}
            tabIndex={tabStop ? 0 : -1}
            disabled={disabled || option.disabled}
            className={[styles.pill, selected ? styles.selected : ''].filter(Boolean).join(' ')}
            onClick={() => onChange(option.value)}
          >
            {option.icon ? <Icon name={option.icon} size={iconSize} strokeWidth={iconStrokeWidth} className={styles.icon} /> : null}
            {showTitle ? <span className={styles.title}>{option.label}</span> : null}
            {option.badge ? <span className={styles.badge}>{option.badge}</span> : null}
          </button>
        )
        // Every pill has a tooltip, like `.help("Chat (⌘1)")` on TabPill.
        return (
          <Tooltip key={option.value} content={option.label} shortcut={option.shortcut} placement="bottom">
            {pill}
          </Tooltip>
        )
      })}
    </div>
  )
}
