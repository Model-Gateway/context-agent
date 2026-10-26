import type { ReactNode } from 'react'
import styles from './ConnectButton.module.css'

export interface ConnectButtonProps {
  /** Solid `textPrimary` (Approve / Save / Connect) or outlined `surface` (Cancel). */
  prominent: boolean
  disabled?: boolean
  onClick: () => void
  children: ReactNode
}

/**
 * `ConnectButtonStyle` (Views/Apps/AppsView.swift): the connect pages'
 * full-width 40 px buttons — outlined Cancel, solid Approve — 10 px
 * radius, 14 px medium text, dimmed while pressed or disabled.
 */
export function ConnectButton({ prominent, disabled = false, onClick, children }: ConnectButtonProps) {
  return (
    <button
      type="button"
      className={[styles.button, prominent ? styles.prominent : styles.outlined].join(' ')}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  )
}
