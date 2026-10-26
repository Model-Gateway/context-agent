import type { ReactNode } from 'react'
import { formatShortcut } from './platform'
import styles from './Kbd.module.css'

export interface KbdProps {
  /** A shortcut spelled with `mod` ("mod+K"): rendered ⌘K on macOS, Ctrl+K elsewhere. */
  shortcut?: string
  /** Literal content instead of `shortcut`. */
  children?: ReactNode
  className?: string
  /** Hidden from assistive technology by default (the hint repeats the tooltip). */
  'aria-hidden'?: boolean
}

/**
 * The `.kbd()` modifier of the macOS app: a keyboard-shortcut hint ("⌘K")
 * in a small raised capsule. `mod` becomes ⌘ or Ctrl by platform.
 */
export function Kbd({ shortcut, children, className, ...aria }: KbdProps) {
  const hidden = aria['aria-hidden'] ?? true
  return (
    <kbd className={[styles.kbd, className ?? ''].filter(Boolean).join(' ')} aria-hidden={hidden || undefined}>
      {shortcut ? formatShortcut(shortcut) : children}
    </kbd>
  )
}
