import { Icon, type IconName } from '../theme/icons'
import { IconButton } from './IconButton'
import type { Toast as ToastModel, ToastLevel } from './toasts'
import styles from './Toast.module.css'

const LEVEL_ICONS: Record<ToastLevel, IconName> = {
  info: 'info',
  success: 'check',
  warning: 'warning',
  error: 'error',
}

export interface ToastProps {
  toast: ToastModel
  onDismiss: (id: string) => void
  onDetails?: (toast: ToastModel) => void
}

/**
 * `ToastView` of the macOS MainWindow: a `surface` card (hairline, 12 px
 * radius, floating shadow, at most 380 px wide) with the level's glyph,
 * the message on up to three selectable lines, an optional "Details" link
 * and a dismiss button.
 */
export function Toast({ toast, onDismiss, onDetails }: ToastProps) {
  const hasDetails = !!toast.details && toast.details.trim().length > 0
  return (
    <div className={styles.toast} role={toast.level === 'error' || toast.level === 'warning' ? 'alert' : 'status'} aria-label={toast.message}>
      <Icon name={LEVEL_ICONS[toast.level]} size={14} strokeWidth={2} className={`${styles.icon} ${styles[toast.level]}`} />
      <div className={`${styles.message} selectable`}>{toast.message}</div>
      {hasDetails && onDetails ? (
        <button type="button" className={styles.details} onClick={() => onDetails(toast)}>
          Details
        </button>
      ) : null}
      <IconButton icon="close" label="Dismiss" size={11} onClick={() => onDismiss(toast.id)} />
    </div>
  )
}
