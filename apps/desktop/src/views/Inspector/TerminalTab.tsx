import { lazy, Suspense, useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { ContextMenu } from '../../components/ContextMenu'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { IconButton } from '../../components/IconButton'
import { Tooltip } from '../../components/Tooltip'
import { prefersReducedMotion } from '../../components/overlays'
import { useApp, useSelectedProject } from '../../state/app'
import { useTerminalSession, useTerminalWorkspace, type TerminalSession, type TerminalWorkspace, type TerminalWorkspaceSnapshot } from '../../state/terminal'
import { Icon } from '../../theme/icons'
import { MiddleText } from './MiddleText'
import { cx, revealInStrip } from './inspectorShared'
import styles from './TerminalTab.module.css'

/** The xterm.js view (TerminalHost.tsx), loaded with the first shell shown. */
const TerminalHost = lazy(() => import('./TerminalHost').then((module) => ({ default: module.TerminalHost })))

/**
 * Port of `TerminalTab` (Views/Inspector/TerminalTab.swift): the selected
 * project's `TerminalWorkspace` — a horizontally scrolling strip of shell
 * tabs ending in a "+" button, over the selected shell's xterm.js view
 * (the SwiftTerm `LocalProcessTerminalView` of the macOS app). Each shell
 * is the login shell (PowerShell on Windows) in the project root; its view
 * lives on the session, so switching tabs keeps scrollback and running
 * programs, and a shell that exits closes its tab.
 */
export function TerminalTab() {
  const project = useSelectedProject()
  if (!project) return <EmptyPlaceholder icon="terminal" title="No project open" subtitle="Open a project to get a shell in its root." />
  return <TerminalPane key={project.id} workspace={useApp.getState().terminals(project)} />
}

/** `TerminalPane`: the strip over the selected session; opens the first shell on appear. */
function TerminalPane({ workspace }: { workspace: TerminalWorkspace }) {
  const snapshot = useTerminalWorkspace(workspace)

  useEffect(() => {
    workspace.startIfNeeded()
  }, [workspace])

  const selected = snapshot?.selected ?? null
  return (
    <div className={styles.pane}>
      {snapshot ? <TerminalTabStrip workspace={workspace} snapshot={snapshot} /> : null}
      {selected ? (
        <div className={styles.hostFrame}>
          <Suspense fallback={<div className={styles.host} aria-busy="true" />}>
            <TerminalHost session={selected} workspace={workspace} />
          </Suspense>
        </div>
      ) : (
        <EmptyPlaceholder
          icon="terminal"
          title="No terminals"
          subtitle="Open a shell in the project root."
          actionTitle="New terminal"
          onAction={() => workspace.newSession()}
        />
      )}
    </div>
  )
}

// MARK: - Tab strip

/** `TerminalTabStrip`: 36 px, chips 2 px apart, the selection kept in view. */
function TerminalTabStrip({ workspace, snapshot }: { workspace: TerminalWorkspace; snapshot: TerminalWorkspaceSnapshot }) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const newButtonRef = useRef<HTMLButtonElement>(null)
  const selectedId = snapshot.selected?.id ?? null

  useEffect(() => {
    const strip = scrollRef.current
    if (selectedId === null || !strip) return
    const chip = strip.querySelector<HTMLElement>(`[data-session-id="${CSS.escape(selectedId)}"]`)
    if (chip) revealInStrip(strip, chip, 'smooth')
  }, [selectedId])

  const newTerminal = () => {
    workspace.newSession()
    // `proxy.scrollTo(newButtonId, anchor: .trailing)` once the new chip is in.
    requestAnimationFrame(() => {
      const strip = scrollRef.current
      const button = newButtonRef.current
      if (strip && button) revealInStrip(strip, button, 'smooth')
    })
  }

  return (
    <div className={styles.strip}>
      <div ref={scrollRef} className={styles.stripScroll}>
        <div className={styles.tabList} role="tablist" aria-label="Terminal tabs">
          {snapshot.sessions.map((session) => (
            <TerminalTabChip
              key={session.id}
              session={session}
              selected={session.id === selectedId}
              onSelect={() => workspace.select(session.id)}
              onClose={() => workspace.close(session)}
            />
          ))}
        </div>
        <IconButton ref={newButtonRef} icon="newChat" label="New terminal" size={12} className={styles.newButton} onClick={newTerminal} />
      </div>
    </div>
  )
}

/** How long a closed chip takes to leave (`withAnimation(.easeOut(duration: 0.15))`). */
const CHIP_CLOSE_MS = 150

/** `TerminalTabChip`: terminal glyph, title (OSC 0/2), close button shown on hover and when selected. */
function TerminalTabChip({ session, selected, onSelect, onClose }: { session: TerminalSession; selected: boolean; onSelect: () => void; onClose: () => void }) {
  const state = useTerminalSession(session)
  const [overClose, setOverClose] = useState(false)
  const [closing, setClosing] = useState(false)
  const title = state?.displayTitle ?? session.displayTitle
  const help = state?.currentDirectory ?? title

  // The chip scales down and fades (its removal transition), then the session closes.
  const onCloseRef = useRef(onClose)
  onCloseRef.current = onClose
  useEffect(() => {
    if (!closing) return
    const timer = window.setTimeout(() => onCloseRef.current(), CHIP_CLOSE_MS)
    return () => window.clearTimeout(timer)
  }, [closing])

  const close = () => {
    if (prefersReducedMotion()) onClose()
    else setClosing(true)
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      onSelect()
    }
  }

  return (
    <ContextMenu items={[{ label: 'Close Terminal', onSelect: close }]}>
      <Tooltip content={help} disabled={overClose}>
        <div
          role="tab"
          tabIndex={0}
          aria-selected={selected}
          aria-label={title}
          data-session-id={session.id}
          className={cx(styles.chip, selected && styles.selected, closing && styles.closing)}
          onClick={onSelect}
          onKeyDown={onKeyDown}
        >
          <Icon name="terminal" size={10} strokeWidth={2} className={styles.chipIcon} />
          <MiddleText text={title} maxWidth={140} className={styles.chipTitle} />
          <Tooltip content="Close terminal">
            <button
              type="button"
              className={styles.close}
              aria-label={`Close ${title}`}
              onPointerEnter={() => setOverClose(true)}
              onPointerLeave={() => setOverClose(false)}
              onClick={(event) => {
                event.stopPropagation()
                close()
              }}
            >
              <Icon name="close" size={8} strokeWidth={3} />
            </button>
          </Tooltip>
        </div>
      </Tooltip>
    </ContextMenu>
  )
}
