import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Tooltip } from '../components/Tooltip'
import { Icon, type IconName } from '../theme/icons'
import styles from './WindowControls.module.css'

/**
 * The caption buttons at the top bar's right end on Windows and Linux,
 * where the window has no native title bar (`decorations: false`):
 * minimize, maximize / restore and close, each a 46 × 52 px hit area over
 * the full bar height; close turns `danger` with a white glyph on hover.
 * macOS keeps its traffic lights (the top bar leaves room for them), so
 * there is no Swift counterpart beyond `TrafficLights` in MainWindow.swift.
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null
    const appWindow = getCurrentWindow()
    const refresh = () => {
      appWindow
        .isMaximized()
        .then((value) => {
          if (!disposed) setMaximized(value)
        })
        .catch(() => undefined)
    }
    refresh()
    appWindow
      .onResized(refresh)
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  const appWindow = () => getCurrentWindow()
  return (
    <div className={styles.controls} role="group" aria-label="Window controls">
      <CaptionButton icon="windowMinimize" label="Minimize" onClick={() => void appWindow().minimize()} />
      <CaptionButton
        icon={maximized ? 'windowRestore' : 'windowMaximize'}
        label={maximized ? 'Restore Down' : 'Maximize'}
        onClick={() => void appWindow().toggleMaximize()}
      />
      <CaptionButton icon="close" label="Close" danger onClick={() => void appWindow().close()} />
    </div>
  )
}

/// The Windows caption glyphs are 10 px with 1 px lines; each glyph gets the
/// Icon size and stroke that land there (the Icon draws lucide's 20-unit
/// live area into its box, so the stroke is in lucide's 24-unit grid).
const GLYPHS: Partial<Record<IconName, { size: number; strokeWidth: number }>> = {
  windowMinimize: { size: 11.7, strokeWidth: 1.7 },
  windowMaximize: { size: 10.8, strokeWidth: 1.85 },
  windowRestore: { size: 10.8, strokeWidth: 1.85 },
  close: { size: 16.7, strokeWidth: 1.2 },
}

function CaptionButton({ icon, label, danger = false, onClick }: { icon: IconName; label: string; danger?: boolean; onClick: () => void }) {
  return (
    <Tooltip content={label} placement="bottom" delay={700}>
      <button type="button" className={[styles.button, danger ? styles.close : ''].filter(Boolean).join(' ')} aria-label={label} onClick={onClick}>
        <Icon name={icon} size={GLYPHS[icon]?.size ?? 10.8} strokeWidth={GLYPHS[icon]?.strokeWidth ?? 1.85} />
      </button>
    </Tooltip>
  )
}
