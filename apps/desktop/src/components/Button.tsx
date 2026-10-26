import type { ButtonHTMLAttributes, ReactNode, Ref } from 'react'
import { Icon, type IconName } from '../theme/icons'
import { BrailleSpinner } from './BrailleSpinner'
import styles from './Button.module.css'

export type ButtonVariant = 'plain' | 'bordered' | 'prominent' | 'destructive'
export type ButtonSize = 'small' | 'regular' | 'large'

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  /**
   * `.plain` (text only), `.bordered` (grey rounded, the default), `.borderedProminent`
   * (accent fill) or a destructive bordered button in `danger`.
   */
  variant?: ButtonVariant
  /** macOS `.controlSize`: small 20 px, regular 24 px (the default), large 30 px. */
  size?: ButtonSize
  /** An icon before the label, at 14 px (12 px when small). */
  icon?: IconName
  /** Shows a `BrailleSpinner` in place of the icon and disables the button. */
  loading?: boolean
  /** Stretches to the container's width. */
  block?: boolean
  children?: ReactNode
  ref?: Ref<HTMLButtonElement>
}

/**
 * SwiftUI `Button` with the app's button styles: `.plain`, `.bordered`,
 * `.borderedProminent` (accent) and a destructive variant, in the three
 * control sizes.
 */
export function Button({
  variant = 'bordered',
  size = 'regular',
  icon,
  loading = false,
  block = false,
  className,
  disabled,
  children,
  type = 'button',
  ref,
  ...rest
}: ButtonProps) {
  const iconSize = size === 'small' ? 12 : 14
  const classes = [styles.button, styles[variant], styles[size], block ? styles.block : '', className ?? '']
    .filter(Boolean)
    .join(' ')
  return (
    <button ref={ref} type={type} className={classes} disabled={disabled || loading} aria-busy={loading || undefined} {...rest}>
      {loading ? (
        <BrailleSpinner size={iconSize} tint="inherit" />
      ) : icon ? (
        <Icon name={icon} size={iconSize} strokeWidth={2} className={styles.icon} />
      ) : null}
      {children !== undefined && children !== null ? <span className={styles.label}>{children}</span> : null}
    </button>
  )
}
