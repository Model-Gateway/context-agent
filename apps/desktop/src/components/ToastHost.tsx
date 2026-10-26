import { Toast } from './Toast'
import { useReducedMotion } from './BrailleSpinner'
import { useToasts } from './toasts'
import styles from './ToastHost.module.css'

/**
 * `ToastOverlay` of the macOS MainWindow: the visible toasts stacked in the
 * window's bottom-right corner (16 px inset, 8 px apart, newest at the
 * bottom), each moving in from the bottom edge and fading in over 0.2 s
 * (fade only with Reduce Motion). Mount once in MainWindow.
 */
export function ToastHost() {
  const toasts = useToasts((state) => state.toasts)
  const dismiss = useToasts((state) => state.dismiss)
  const showDetails = useToasts((state) => state.showDetails)
  const reduced = useReducedMotion()
  return (
    <div className={styles.host} aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={[styles.slot, reduced ? styles.fade : styles.slide].join(' ')}>
          <Toast toast={toast} onDismiss={dismiss} onDetails={showDetails} />
        </div>
      ))}
    </div>
  )
}
