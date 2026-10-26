import { Icon, type IconName } from '../theme/icons'
import { Button } from './Button'
import styles from './EmptyPlaceholder.module.css'

export interface EmptyPlaceholderProps {
  icon: IconName
  title: string
  subtitle?: string
  actionTitle?: string
  onAction?: () => void
  className?: string
}

/**
 * `EmptyPlaceholder` of the macOS app: a centered icon, title and subtitle
 * with an optional bordered action button, for empty and error states
 * ("Working tree clean", "No tasks yet").
 */
export function EmptyPlaceholder({ icon, title, subtitle, actionTitle, onAction, className }: EmptyPlaceholderProps) {
  return (
    <div className={[styles.placeholder, className ?? ''].filter(Boolean).join(' ')} role="group" aria-label={title}>
      <Icon name={icon} size={28} strokeWidth={1.5} className={styles.icon} />
      <div className={styles.title}>{title}</div>
      {subtitle ? <div className={styles.subtitle}>{subtitle}</div> : null}
      {actionTitle && onAction ? (
        <Button variant="bordered" className={styles.action} onClick={onAction}>
          {actionTitle}
        </Button>
      ) : null}
    </div>
  )
}
