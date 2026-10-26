import type { CSSProperties, MouseEvent, ReactNode } from 'react'
import { Icon, type IconName } from '../theme/icons'
import styles from './Chip.module.css'

export type ChipVariant = 'neutral' | 'tinted' | 'badge'

export interface ChipProps {
  /** The label (one line). */
  children: ReactNode
  /** A glyph before the text at 10 px (9 px for badges). */
  icon?: IconName
  /** `.neutral` grey, `.tinted(color)` (see `tint`) or the attention `.badge`. */
  variant?: ChipVariant
  /** The colour of a tinted chip, any CSS colour (`var(--accent)`); text in it, background at 14 %. */
  tint?: string
  /** Makes the chip a button. */
  onClick?: (event: MouseEvent<HTMLElement>) => void
  /** Accessible name when the text alone does not say enough. */
  'aria-label'?: string
  title?: string
  className?: string
  style?: CSSProperties
}

/**
 * `Chip` of the macOS app: a small rounded label, a neutral grey chip
 * ("script", "main"), a tinted one (status, counts) or an attention badge
 * ("New", unseen counts).
 */
export function Chip({ children, icon, variant = 'neutral', tint, onClick, className, style, title, ...aria }: ChipProps) {
  const isBadge = variant === 'badge'
  const classes = [styles.chip, styles[variant], onClick ? styles.clickable : '', className ?? ''].filter(Boolean).join(' ')
  const merged: CSSProperties = variant === 'tinted' && tint ? { ...style, ['--tint' as string]: tint } : (style ?? {})
  const content = (
    <>
      {icon ? <Icon name={icon} size={isBadge ? 9 : 10} strokeWidth={2.25} className={styles.icon} /> : null}
      <span className={styles.text}>{children}</span>
    </>
  )
  if (onClick) {
    return (
      <button type="button" className={classes} style={merged} onClick={onClick} title={title} aria-label={aria['aria-label']}>
        {content}
      </button>
    )
  }
  return (
    <span className={classes} style={merged} title={title} aria-label={aria['aria-label']}>
      {content}
    </span>
  )
}
