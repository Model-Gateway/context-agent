import { useEffect, useRef, useState, type FormEvent } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { IconButton } from '../../components/IconButton'
import { Tooltip } from '../../components/Tooltip'
import { onOverlayStackChange, openOverlayElements } from '../../components/overlays'
import { useApp, useSelectedProject } from '../../state/app'
import { useBrowserWorkspace, type BrowserSnapshot, type BrowserWorkspace } from '../../state/browser'
import { Icon } from '../../theme/icons'
import { MiddleText } from './MiddleText'
import { cx, useInspectorPaneActive } from './inspectorShared'
import styles from './BrowserTab.module.css'

/**
 * Port of `BrowserTab` (Views/Inspector/BrowserTab.swift): the selected
 * project's `BrowserWorkspace` — a toolbar (back, forward, reload / stop,
 * address field that also searches, open in the default browser) with a
 * thin progress bar, over the project's long-lived page (a native child
 * webview laid over the tab's area). The agent drives the same page through
 * the `browser` tool; while it does, an "Agent · …" line says what it is
 * doing (kept 4 s after the action), but the Inspector never switches to
 * this tab on its own.
 */
export function BrowserTab() {
  const project = useSelectedProject()
  if (!project) return <EmptyPlaceholder icon="browser" title="No project open" subtitle="Open a project to browse alongside it." />
  return <BrowserPane key={project.id} workspace={useApp.getState().browser(project)} />
}

/** `BrowserPane`: toolbar, progress, agent line, load error, then the page (or the placeholder under it). */
function BrowserPane({ workspace }: { workspace: BrowserWorkspace }) {
  const snapshot = useBrowserWorkspace(workspace)
  const active = useInspectorPaneActive()
  const hostRef = useRef<HTMLDivElement>(null)

  // The native page follows the host's area while the pane is the shown
  // one; it is parked while an overlay (menu, popover, sheet) covers it,
  // since a native webview paints above the HTML.
  useEffect(() => {
    const element = hostRef.current
    if (!active || !element) return
    const stop = workspace.track(element)
    let frame = 0
    const check = () => {
      frame = 0
      if (coveredByOverlay(element)) workspace.hide()
      else workspace.show()
    }
    const schedule = () => {
      if (frame === 0) frame = requestAnimationFrame(check)
    }
    const unsubscribe = onOverlayStackChange(schedule)
    check()
    return () => {
      unsubscribe()
      if (frame !== 0) cancelAnimationFrame(frame)
      stop()
    }
  }, [workspace, active])

  if (!snapshot) return null
  const activity = snapshot.agentActivity ?? snapshot.recentAgentAction

  return (
    <div className={styles.pane}>
      <BrowserToolbar workspace={workspace} snapshot={snapshot} />
      {snapshot.isLoading ? <ProgressBar fraction={snapshot.progress} /> : null}
      {activity !== null ? <AgentActivityLine text={activity} live={snapshot.agentActivity !== null} /> : null}
      {snapshot.loadError !== null ? (
        <div className={styles.loadError} role="alert">
          <p className={styles.loadErrorText}>{snapshot.loadError}</p>
        </div>
      ) : null}
      <div className={styles.page}>
        <div ref={hostRef} className={styles.webHost} />
        {!snapshot.hasPage && !snapshot.isLoading ? (
          <div className={styles.placeholder}>
            <EmptyPlaceholder
              icon="browser"
              title="Browser"
              subtitle="Enter an address above, or ask the agent to open a page: it drives this tab without switching to it."
            />
          </div>
        ) : null}
      </div>
    </div>
  )
}

/** Whether an open overlay's box overlaps `element`'s. */
function coveredByOverlay(element: HTMLElement): boolean {
  const overlays = openOverlayElements()
  if (overlays.length === 0) return false
  const area = element.getBoundingClientRect()
  return overlays.some((overlay) => {
    const box = overlay.getBoundingClientRect()
    return box.width > 0 && box.height > 0 && box.left < area.right && box.right > area.left && box.top < area.bottom && box.bottom > area.top
  })
}

// MARK: - Toolbar

/** `BrowserToolbar`: 36 px, 4 px apart, hairline below. */
function BrowserToolbar({ workspace, snapshot }: { workspace: BrowserWorkspace; snapshot: BrowserSnapshot }) {
  return (
    <div className={styles.toolbar} role="toolbar" aria-label="Browser toolbar">
      <IconButton icon="chevronLeft" label="Back" size={12} className={styles.wideGlyph} disabled={!snapshot.canGoBack} onClick={() => void workspace.goBack()} />
      <IconButton
        icon="chevronRight"
        label="Forward"
        size={12}
        className={styles.wideGlyph}
        disabled={!snapshot.canGoForward}
        onClick={() => void workspace.goForward()}
      />
      {snapshot.isLoading ? (
        <IconButton icon="close" label="Stop loading" size={12} className={styles.wideGlyph} onClick={() => void workspace.stop()} />
      ) : (
        <IconButton icon="refresh" label="Reload" size={12} className={styles.roundGlyph} disabled={!snapshot.hasPage} onClick={() => void workspace.reload()} />
      )}
      <AddressField workspace={workspace} snapshot={snapshot} />
      {snapshot.url !== null && snapshot.hasPage ? (
        <IconButton icon="externalLink" label="Open in default browser" size={11} className={styles.arrowGlyph} onClick={() => openExternally(snapshot.url)} />
      ) : null}
    </div>
  )
}

/** `NSWorkspace.shared.open(url)`: the page in the default browser. */
function openExternally(url: string | null): void {
  if (url === null) return
  invoke('open_url', { url }).catch(() => {
    /* Outside Tauri there is nothing to open with. */
  })
}

/** The address field: lock (https) or globe, the address or search words; Return opens it. */
function AddressField({ workspace, snapshot }: { workspace: BrowserWorkspace; snapshot: BrowserSnapshot }) {
  const inputRef = useRef<HTMLInputElement>(null)
  const [focused, setFocused] = useState(false)
  const secure = snapshot.url !== null && snapshot.url.startsWith('https:')

  const submit = (event: FormEvent) => {
    event.preventDefault()
    void workspace.open(workspace.addressText)
    inputRef.current?.blur()
  }

  return (
    <Tooltip content={snapshot.title.length === 0 ? 'Address' : snapshot.title}>
      <form className={cx(styles.address, focused && styles.addressFocused)} onSubmit={submit}>
        <Icon name={secure ? 'lock' : 'browser'} size={10} strokeWidth={2} className={styles.addressIcon} />
        <input
          ref={inputRef}
          className={styles.addressInput}
          type="text"
          value={snapshot.addressText}
          placeholder="Address or search"
          aria-label="Address"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          onChange={(event) => workspace.setAddressText(event.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
        />
      </form>
    </Tooltip>
  )
}

/** `ProgressBar`: the thin accent bar under the toolbar while a page loads. */
function ProgressBar({ fraction }: { fraction: number }) {
  return (
    <div className={styles.progress} aria-hidden="true">
      <div className={styles.progressFill} style={{ width: `max(8px, ${Math.max(0.05, fraction) * 100}%)` }} />
    </div>
  )
}

/** `AgentActivityLine`: "Agent · Opening example.com…" while the agent acts, then its last action for a moment. */
function AgentActivityLine({ text, live }: { text: string; live: boolean }) {
  return (
    <div className={cx(styles.activity, live && styles.activityLive)} role="status">
      {live ? <BrailleSpinner size={10} tint="var(--accent)" /> : <Icon name="check" size={10} strokeWidth={2} className={styles.activityIcon} />}
      <MiddleText text={`Agent · ${text}`} grow className={styles.activityText} />
    </div>
  )
}
