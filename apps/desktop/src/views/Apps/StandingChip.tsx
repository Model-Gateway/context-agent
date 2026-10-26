import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import type { AppStanding } from './standing'
import styles from './StandingChip.module.css'

export interface StandingChipProps {
  standing: AppStanding
}

/**
 * `StandingChip` (Views/Apps/AppsView.swift): "Connected", "Connecting",
 * "Failed", "Paused", "Not connected" — the status word in text, tinted.
 */
export function StandingChip({ standing }: StandingChipProps) {
  switch (standing.kind) {
    case 'connected':
    case 'installed':
      return (
        <Chip variant="tinted" tint="var(--success)">
          Connected
        </Chip>
      )
    case 'connecting':
      return (
        <span className={styles.connecting}>
          <BrailleSpinner size={11} />
          <span className={styles.label}>Connecting</span>
        </span>
      )
    case 'failed':
      return (
        <Chip variant="tinted" tint="var(--danger)" icon="warning">
          Failed
        </Chip>
      )
    case 'off':
      return <Chip>Paused</Chip>
    case 'notInstalled':
      return <Chip>Not connected</Chip>
  }
}
