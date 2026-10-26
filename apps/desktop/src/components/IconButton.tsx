import type { ButtonHTMLAttributes, Ref } from 'react'
import { Icon, type IconName } from '../theme/icons'
import { Tooltip } from './Tooltip'
import styles from './IconButton.module.css'

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'aria-label' | 'children'> {
  icon: IconName
  /** The tooltip and the accessible name; required. */
  label: string
  /** Tints the glyph `accent` and keeps the highlight on (inspector toggle). */
  isActive?: boolean
  /** Glyph size; the button is `size + 12` square. 16 by default. */
  size?: number
  /** A shortcut shown in the tooltip, spelled for the platform ("mod+shift+I"). */
  shortcut?: string
  /** Hides the tooltip (the label stays as the accessible name). */
  noTooltip?: boolean
  /** Tooltip placement. */
  tooltipPlacement?: 'top' | 'bottom'
  ref?: Ref<HTMLButtonElement>
}

/**
 * `IconButton` of the macOS app: a borderless glyph button with a hover
 * highlight, a tooltip and a VoiceOver label (the same `label`).
 */
export function IconButton({
  icon,
  label,
  isActive = false,
  size = 16,
  shortcut,
  noTooltip = false,
  tooltipPlacement,
  className,
  ref,
  ...rest
}: IconButtonProps) {
  const button = (
    <button
      ref={ref}
      type="button"
      aria-label={label}
      aria-pressed={isActive || undefined}
      className={[styles.button, isActive ? styles.active : '', className ?? ''].filter(Boolean).join(' ')}
      style={{ width: size + 12, height: size + 12 }}
      {...rest}
    >
      <Icon name={icon} size={size} strokeWidth={2} />
    </button>
  )
  if (noTooltip) return button
  return (
    <Tooltip content={label} shortcut={shortcut} placement={tooltipPlacement}>
      {button}
    </Tooltip>
  )
}
