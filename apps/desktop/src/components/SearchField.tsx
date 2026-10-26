import { useId, useState, type ChangeEvent, type KeyboardEvent, type Ref } from 'react'
import { Icon } from '../theme/icons'
import { Kbd } from './Kbd'
import styles from './SearchField.module.css'

export interface SearchFieldProps {
  placeholder: string
  value: string
  onChange: (value: string) => void
  /** A shortcut shown while the field is empty ("mod+K"), spelled for the platform. */
  kbdHint?: string
  /** Return pressed. */
  onSubmit?: () => void
  /** Esc pressed on an empty field (an escape from a non-empty one clears it first). */
  onEscape?: () => void
  /** Extra key handling (arrow navigation in lists); runs before the field's own. */
  onKeyDown?: (event: KeyboardEvent<HTMLInputElement>) => void
  autoFocus?: boolean
  disabled?: boolean
  /** Accessible name; the placeholder by default. */
  'aria-label'?: string
  className?: string
  ref?: Ref<HTMLInputElement>
}

/**
 * `SearchField` of the macOS app: a rounded field with a magnifier, a
 * placeholder, a clear button while there is text and an optional keyboard
 * hint ("⌘K") while it is empty. The border turns `borderStrong` on focus.
 */
export function SearchField({
  placeholder,
  value,
  onChange,
  kbdHint,
  onSubmit,
  onEscape,
  onKeyDown,
  autoFocus,
  disabled,
  className,
  ref,
  ...aria
}: SearchFieldProps) {
  const [focused, setFocused] = useState(false)
  const id = useId()

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    onKeyDown?.(event)
    if (event.defaultPrevented) return
    if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
      if (onSubmit) {
        event.preventDefault()
        onSubmit()
      }
    } else if (event.key === 'Escape') {
      if (value) {
        event.preventDefault()
        event.stopPropagation()
        onChange('')
      } else if (onEscape) {
        event.preventDefault()
        event.stopPropagation()
        onEscape()
      }
    }
  }

  return (
    <div className={[styles.field, focused ? styles.focused : '', disabled ? styles.disabled : '', className ?? ''].filter(Boolean).join(' ')}>
      <Icon name="search" size={12} strokeWidth={2.25} className={styles.magnifier} />
      <input
        ref={ref}
        id={id}
        type="text"
        className={styles.input}
        placeholder={placeholder}
        aria-label={aria['aria-label'] ?? placeholder}
        value={value}
        disabled={disabled}
        autoFocus={autoFocus}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        onChange={(event: ChangeEvent<HTMLInputElement>) => onChange(event.target.value)}
        onKeyDown={handleKeyDown}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
      />
      {value ? (
        <button type="button" className={styles.clear} aria-label="Clear search" title="Clear" onClick={() => onChange('')} tabIndex={-1}>
          <Icon name="clear" size={12} />
        </button>
      ) : kbdHint ? (
        <Kbd shortcut={kbdHint} />
      ) : null}
    </div>
  )
}
