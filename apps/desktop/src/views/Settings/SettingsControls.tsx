// The SwiftUI controls the Settings forms are built from, as web controls
// styled like their macOS counterparts: `Toggle` with `.toggleStyle(.switch)`
// and `.controlSize(.small)`, `TextField` / `SecureField` with
// `.settingsField()` (rounded border, the secondary font), `Picker` in its
// menu style (a pop-up button), `Picker` with `.pickerStyle(.radioGroup)`,
// and the Save · Revert · spinner · "Unsaved changes" row every form ends
// with. Only the Settings views use them (apps/macos/ContextAgent/Views/Settings/*).

import { useId, useRef, useState, type ReactNode } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Menu, type MenuItem } from '../../components/Menu'
import { SegmentedControl } from '../../components/SegmentedControl'
import { Switch } from '../../components/Switch'
import { Tooltip } from '../../components/Tooltip'
import styles from './SettingsControls.module.css'

// MARK: - Toggle

export interface ToggleProps {
  /** The label; with `labelHidden` only the accessible name (`.labelsHidden()`). */
  label: string
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  /** `.labelsHidden()`: the switch alone. */
  labelHidden?: boolean
}

/**
 * `Toggle(label, isOn:)` with `.toggleStyle(.switch)` and
 * `.controlSize(.small)`: the label, then a small switch (accent when on).
 */
export function Toggle({ label, checked, onChange, disabled = false, labelHidden = false }: ToggleProps) {
  if (labelHidden) return <Switch label={label} checked={checked} disabled={disabled} onChange={onChange} />
  return (
    <label className={[styles.toggle, disabled ? styles.disabled : ''].filter(Boolean).join(' ')}>
      <span className={styles.toggleLabel} aria-hidden="true">
        {label}
      </span>
      <Switch label={label} checked={checked} disabled={disabled} onChange={onChange} className={styles.labelledSwitch} />
    </label>
  )
}

// MARK: - Text fields

export interface TextFieldProps {
  value: string
  onChange: (value: string) => void
  /** The SwiftUI title: the placeholder (prompt). */
  placeholder: string
  /** The accessible name; the placeholder by default (SwiftUI uses the title). */
  'aria-label'?: string
  /** `.onSubmit`: Return in the field. */
  onSubmit?: () => void
  /** A fixed width (`.frame(width:)`); the field stretches otherwise. */
  width?: number
  /** `SecureField`. */
  secure?: boolean
  disabled?: boolean
  className?: string
}

/**
 * `TextField(title, text:)` / `SecureField` with `.settingsField()`: a
 * rounded-border field in the secondary font, regular control size.
 */
export function TextField({ value, onChange, placeholder, onSubmit, width, secure = false, disabled = false, className, ...aria }: TextFieldProps) {
  return (
    <input
      type={secure ? 'password' : 'text'}
      className={[styles.field, width === undefined ? styles.stretch : '', className ?? ''].filter(Boolean).join(' ')}
      style={width === undefined ? undefined : { width }}
      value={value}
      placeholder={placeholder}
      aria-label={aria['aria-label'] ?? placeholder}
      disabled={disabled}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
      autoComplete={secure ? 'new-password' : 'off'}
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && !event.nativeEvent.isComposing && onSubmit) {
          event.preventDefault()
          onSubmit()
        }
      }}
    />
  )
}

// MARK: - Pop-up button

/** One entry of a `PopUpButton`: a choice or a divider. */
export type PopUpOption<T extends string> = { value: T; label: string } | { divider: true }

export interface PopUpButtonProps<T extends string> {
  value: T
  options: ReadonlyArray<PopUpOption<T>>
  onChange: (value: T) => void
  'aria-label': string
  disabled?: boolean
  /** `.controlSize(.small)`. */
  small?: boolean
  /// `.frame(maxWidth:)`: an AppKit pop-up button fills the width it is
  /// offered, so the button is this wide unless the row is narrower.
  maxWidth?: number
  /** Shown when `value` is not among the options. */
  fallbackLabel?: string
}

/**
 * `Picker` in its default (menu) style: a pop-up button showing the current
 * choice; a click opens the choices with a checkmark on the current one.
 */
export function PopUpButton<T extends string>({ value, options, onChange, disabled = false, small = false, maxWidth, fallbackLabel, ...aria }: PopUpButtonProps<T>) {
  const anchorRef = useRef<HTMLButtonElement>(null)
  const [open, setOpen] = useState(false)
  const current = options.find((option): option is { value: T; label: string } => !('divider' in option) && option.value === value)
  const items: MenuItem[] = options.map((option, index) =>
    'divider' in option
      ? { type: 'separator', id: `divider-${index}` }
      : { id: `option-${option.value}`, label: option.label, checked: option.value === value, onSelect: () => onChange(option.value) },
  )
  return (
    <>
      <button
        ref={anchorRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`${aria['aria-label']}: ${current?.label ?? fallbackLabel ?? value}`}
        disabled={disabled}
        className={[styles.popUp, small ? styles.popUpSmall : '', maxWidth === undefined ? '' : styles.popUpFill].filter(Boolean).join(' ')}
        style={maxWidth === undefined ? undefined : { width: maxWidth }}
        onClick={() => setOpen((shown) => !shown)}
      >
        <span className={styles.popUpLabel}>{current?.label ?? fallbackLabel ?? value}</span>
        <PopUpIndicator small={small} />
      </button>
      <Menu open={open} onClose={() => setOpen(false)} items={items} anchorRef={anchorRef} aria-label={aria['aria-label']} />
    </>
  )
}

/// The pop-up button's indicator, AppKit's "chevron.up.chevron.down" (a
/// pull-down menu has a single chevron; a pop-up shows both, since its menu
/// opens over the button around the current choice).
function PopUpIndicator({ small }: { small: boolean }) {
  const height = small ? 10 : 11
  return (
    <svg className={styles.popUpChevron} width={7} height={height} viewBox="0 0 7 11" aria-hidden="true">
      <path d="M1 4 3.5 1.5 6 4M1 7l2.5 2.5L6 7" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

// MARK: - Segmented picker

export interface SegmentedPickerProps<T extends string> {
  value: T
  options: ReadonlyArray<{ value: T; label: string }>
  onChange: (value: T) => void
  'aria-label': string
  /** `.frame(maxWidth:)`: the control stretches to this width, the segments share it equally. */
  maxWidth?: number
  disabled?: boolean
}

/**
 * `Picker` with `.pickerStyle(.segmented)` and `.labelsHidden()`: the shared
 * NSSegmentedControl (components/SegmentedControl), stretched to `maxWidth`.
 */
export function SegmentedPicker<T extends string>({ value, options, onChange, maxWidth, disabled = false, ...aria }: SegmentedPickerProps<T>) {
  return <SegmentedControl options={options} value={value} onChange={onChange} maxWidth={maxWidth ?? Number.POSITIVE_INFINITY} disabled={disabled} aria-label={aria['aria-label']} />
}

// MARK: - Radio group

export interface RadioGroupProps<T extends string> {
  value: T
  options: ReadonlyArray<{ value: T; label: string }>
  onChange: (value: T) => void
  'aria-label': string
  disabled?: boolean
}

/** `Picker` with `.pickerStyle(.radioGroup)` and `.labelsHidden()`: one radio button per choice, stacked. */
export function RadioGroup<T extends string>({ value, options, onChange, disabled = false, ...aria }: RadioGroupProps<T>) {
  const name = useId()
  return (
    <div role="radiogroup" aria-label={aria['aria-label']} aria-disabled={disabled || undefined} className={[styles.radioGroup, disabled ? styles.disabled : ''].filter(Boolean).join(' ')}>
      {options.map((option) => (
        <label key={option.value} className={styles.radio}>
          <input
            type="radio"
            name={name}
            className={styles.radioInput}
            checked={option.value === value}
            disabled={disabled}
            onChange={() => onChange(option.value)}
          />
          <span className={styles.radioMark} aria-hidden="true" />
          <span className={styles.radioLabel}>{option.label}</span>
        </label>
      ))}
    </div>
  )
}

// MARK: - Save row

export interface FormButtonsProps {
  onSave: () => void
  onRevert: () => void
  /** Unsaved edits: enables both buttons and shows "Unsaved changes". */
  dirty: boolean
  saving: boolean
  /** The backend is connected (Save needs it). */
  ready: boolean
  /** `.keyboardShortcut(.defaultAction)` on Save: the accent default button. */
  defaultAction?: boolean
  /** Controls before Save ("Add model limit", "Show all … models"). */
  leading?: ReactNode
}

/**
 * The row every Settings form ends with: Save, Revert, a spinner while
 * saving, and "Unsaved changes" at the trailing edge while the form is
 * dirty (`.controlSize(.small)`).
 */
export function FormButtons({ onSave, onRevert, dirty, saving, ready, defaultAction = false, leading }: FormButtonsProps) {
  const saveEnabled = dirty && !saving && ready
  return (
    <div className={styles.formButtons}>
      {leading}
      {/* A disabled default button loses its accent, like AppKit's. */}
      <Button size="small" variant={defaultAction && saveEnabled ? 'prominent' : 'bordered'} disabled={!saveEnabled} onClick={onSave}>
        Save
      </Button>
      <Button size="small" disabled={!dirty || saving} onClick={onRevert}>
        Revert
      </Button>
      {saving ? <BrailleSpinner /> : null}
      <span className={styles.spacer} />
      {dirty && !saving ? <span className={styles.unsaved}>Unsaved changes</span> : null}
    </div>
  )
}

// MARK: - Help

/// `.help(text)` on a control: the tooltip, or the control alone when `text`
/// is empty. The control sits in a host span that takes the pointer while
/// the control is disabled (Chromium sends no pointer events to a disabled
/// button), so the tooltip also explains why it is disabled, like AppKit's.
export function Help({ text, children }: { text: string; children: ReactNode }) {
  if (text.length === 0) return <>{children}</>
  return (
    <Tooltip content={text}>
      <span className={styles.help}>{children}</span>
    </Tooltip>
  )
}
