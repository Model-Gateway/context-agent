import { useCallback, useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { backend } from '../../backend/client'
import type { McpPermissions } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { Menu } from '../../components/Menu'
import { Tooltip } from '../../components/Tooltip'
import { useApp } from '../../state/app'
import { onAppBecameActive } from '../../state/notifications'
import { isReady, useAnyProjectRunning } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { InlineMessage } from '../../components/InlineMessage'
import styles from './ComputerUsePermissions.module.css'

/** The bundle identifier `tccutil reset` needs (tauri.conf.json `identifier`). */
const BUNDLE_ID = 'dev.contextagent.desktop.tauri'

type Permission = 'screenRecording' | 'accessibility'

const PERMISSIONS: readonly Permission[] = ['screenRecording', 'accessibility']

/** `Permission.title`. */
function permissionTitle(permission: Permission): string {
  return permission === 'screenRecording' ? 'Screen Recording' : 'Accessibility'
}

/** `Permission.detail`. */
function permissionDetail(permission: Permission): string {
  return permission === 'screenRecording' ? 'Needed for screenshots.' : 'Needed to move the mouse and type.'
}

/** `Permission.pane`: the System Settings pane. */
function permissionPane(permission: Permission): string {
  return permission === 'screenRecording' ? 'Privacy_ScreenCapture' : 'Privacy_Accessibility'
}

/** `Permission.service`: the `tccutil` service. */
function permissionService(permission: Permission): string {
  return permission === 'screenRecording' ? 'ScreenCapture' : 'Accessibility'
}

/** The backend's answer for `permission` (`mcp/permissions`), null before it answered. */
function backendHas(answer: McpPermissions | null, permission: Permission): boolean | null {
  if (!answer) return null
  return permission === 'screenRecording' ? answer.screen_recording : answer.accessibility
}

/** `openPane(_:)`: System Settings at the permission's Privacy pane. */
function openPane(permission: Permission): void {
  invoke('open_url', { url: `x-apple.systempreferences:com.apple.preference.security?${permissionPane(permission)}` }).catch(() => {
    /* Outside Tauri there is nothing to open with. */
  })
}

/**
 * `ComputerUsePermissions` (Views/Apps/AppsView.swift): Screen Recording
 * and Accessibility for Computer Use on macOS, as the running backend sees
 * them (`mcp/permissions`), re-read every two seconds while the card is
 * visible and whenever the window comes to the front, so a grant shows up
 * without a relaunch. Grant… opens the System Settings pane; the menu also
 * offers Reset (as a `tccutil` command to run).
 */
export function ComputerUsePermissions() {
  const anyProjectRunning = useAnyProjectRunning()
  const backendStarting = useApp((state) => state.backendStarting)
  const restartBackend = useApp((state) => state.restartBackend)
  const [answer, setAnswer] = useState<McpPermissions | null>(null)
  const [working, setWorking] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  const mounted = useRef(true)

  const refresh = useCallback(async () => {
    let next: McpPermissions | null = null
    if (isReady(useApp.getState())) {
      try {
        next = await backend.mcpPermissions()
      } catch {
        next = null
      }
    }
    if (mounted.current) setAnswer(next)
  }, [])

  // Live: grants arrive while the user is in System Settings.
  useEffect(() => {
    mounted.current = true
    let timer: number | null = null
    const loop = async () => {
      await refresh()
      if (mounted.current) timer = window.setTimeout(() => void loop(), 2000)
    }
    void loop()
    const stop = onAppBecameActive(() => void refresh())
    return () => {
      mounted.current = false
      if (timer !== null) window.clearTimeout(timer)
      stop()
    }
  }, [refresh])

  const request = (permission: Permission) => {
    setNote(null)
    openPane(permission)
  }

  /// `reset(_:)`: `tccutil reset <service> <bundle id>` drops the stale
  /// entry a rebuilt (re-signed) app leaves behind. This app cannot run it
  /// itself, so the note gives the command, as the Swift app does for a
  /// build without a bundle identifier.
  const reset = (permission: Permission) => {
    setNote(`Run \`tccutil reset ${permissionService(permission)} ${BUNDLE_ID}\` in Terminal, then grant ${permissionTitle(permission)} again.`)
  }

  const restart = async () => {
    setWorking(true)
    setNote(null)
    await restartBackend()
    await refresh()
    if (mounted.current) setWorking(false)
  }

  return (
    <div className={styles.card}>
      <div className={styles.header}>
        <h2 className={styles.heading}>macOS permissions for Context</h2>
        <span className={styles.spacer} />
        <Tooltip content="Check again">
          <button type="button" className={styles.refresh} aria-label="Check permissions again" onClick={() => void refresh()}>
            <Icon name="refresh" size={13} />
          </button>
        </Tooltip>
      </div>
      {PERMISSIONS.map((permission) => (
        <PermissionRow
          key={permission}
          permission={permission}
          answer={answer}
          working={working}
          restartDisabled={anyProjectRunning || backendStarting}
          restartHelp={anyProjectRunning ? 'Finishes after the running task' : 'Restart ctx so it picks up the permission'}
          onGrant={() => request(permission)}
          onOpenSettings={() => openPane(permission)}
          onReset={() => reset(permission)}
          onRestart={() => void restart()}
        />
      ))}
      {note ? <InlineMessage message={{ kind: 'info', text: note }} /> : null}
      <p className={styles.footnote}>
        Checked live, in Context and in the running backend. Screen Recording only applies to processes started after you grant it, so the backend may
        need a restart; a rebuilt Context can be listed in System Settings yet no longer honoured, which Reset clears so macOS asks again.
      </p>
    </div>
  )
}

interface PermissionRowProps {
  permission: Permission
  answer: McpPermissions | null
  working: boolean
  restartDisabled: boolean
  restartHelp: string
  onGrant: () => void
  onOpenSettings: () => void
  onReset: () => void
  onRestart: () => void
}

/// `permissionRow(_:)`. The app's own answer (CGPreflightScreenCaptureAccess
/// / AXIsProcessTrusted in Swift) is not readable from the web view; macOS
/// attributes the backend's permissions to this app, which launched it, so
/// the backend's answer stands for both.
function PermissionRow({ permission, answer, working, restartDisabled, restartHelp, onGrant, onOpenSettings, onReset, onRestart }: PermissionRowProps) {
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLButtonElement>(null)
  const backendAnswer = backendHas(answer, permission)
  const app = backendAnswer ?? false
  const granted = app && (backendAnswer ?? true)
  const staleBackend = app && backendAnswer === false
  return (
    <div className={styles.row}>
      <span className={styles.rowIcon} style={{ color: granted ? 'var(--success)' : 'var(--warning)' }}>
        <Icon name={granted ? 'checkFill' : 'warningFill'} size={13} />
      </span>
      <div className={styles.rowText}>
        <span className={styles.rowTitle}>{permissionTitle(permission)}</span>
        <span className={styles.rowDetail}>{staleBackend ? 'Granted to Context; the backend started before that.' : permissionDetail(permission)}</span>
      </div>
      <span className={styles.rowSpacer} />
      {working ? (
        <BrailleSpinner size={11} />
      ) : granted ? (
        <Chip variant="tinted" tint="var(--success)" icon="checkmark">
          Granted
        </Chip>
      ) : staleBackend ? (
        <Tooltip content={restartHelp}>
          <Button size="small" disabled={restartDisabled} onClick={onRestart}>
            Restart backend
          </Button>
        </Tooltip>
      ) : (
        <>
          <Button size="small" onClick={onGrant}>
            Grant…
          </Button>
          <button
            ref={menuRef}
            type="button"
            className={styles.more}
            aria-label={`More options for ${permissionTitle(permission)}`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <Icon name="more" size={13} />
          </button>
          <Menu
            open={menuOpen}
            onClose={() => setMenuOpen(false)}
            anchorRef={menuRef}
            align="end"
            aria-label={`More options for ${permissionTitle(permission)}`}
            items={[
              { label: 'Open System Settings', onSelect: onOpenSettings },
              { label: 'Reset and ask again', onSelect: onReset },
            ]}
          />
        </>
      )}
    </div>
  )
}
