import type { CSSProperties } from 'react'
import { Icon, type IconName } from '../theme/icons'
import styles from './InlineMessage.module.css'

/** `SettingsMessage` (Views/Settings/SettingsSections.swift): one line of feedback. */
export interface InlineMessageData {
  kind: 'success' | 'error' | 'info'
  text: string
}

const ICONS: Record<InlineMessageData['kind'], IconName> = { success: 'check', error: 'warning', info: 'info' }
const TINTS: Record<InlineMessageData['kind'], string> = { success: 'var(--success)', error: 'var(--danger)', info: 'var(--text-secondary)' }

/**
 * `InlineMessage` (Views/Settings/SettingsSections.swift), which the Apps
 * pages reuse: a success, error or neutral note under a form, in its tint
 * on an 8 % wash of it, the text selectable.
 */
export function InlineMessage({ message }: { message: InlineMessageData }) {
  const style = { ['--tint' as string]: TINTS[message.kind] } as CSSProperties
  return (
    <div className={styles.message} style={style} role={message.kind === 'error' ? 'alert' : 'status'}>
      <Icon name={ICONS[message.kind]} size={12} strokeWidth={2.25} className={styles.icon} />
      <span className={`selectable ${styles.text}`}>{message.text}</span>
    </div>
  )
}
