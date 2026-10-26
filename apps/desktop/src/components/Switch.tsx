import styles from './Switch.module.css'

export interface SwitchProps {
  checked: boolean
  onChange: (checked: boolean) => void
  /** The accessible name (`Toggle(label)` with `.labelsHidden()`). */
  label: string
  disabled?: boolean
  className?: string
}

/**
 * `Toggle(…).toggleStyle(.switch).controlSize(.small)` of the macOS views:
 * the macOS 26 small NSSwitch (44 × 20 track, a 26 × 16 pill knob inset
 * 2 px, measured from a native render), `accent` when on. Its label is
 * spoken, not shown; Space / Enter toggle it. Settings, Account and Apps
 * all use this one control.
 */
export function Switch({ checked, onChange, label, disabled = false, className }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={[styles.switch, checked ? styles.on : '', className ?? ''].filter(Boolean).join(' ')}
      onClick={() => {
        if (!disabled) onChange(!checked)
      }}
    >
      <span className={styles.knob} aria-hidden="true" />
    </button>
  )
}
