import { useEffect, useState, type AnimationEvent, type ReactNode } from 'react'
import { IconButton } from '../../components/IconButton'
import { Tooltip } from '../../components/Tooltip'
import { prefersReducedMotion } from '../../components/overlays'
import { INSPECTOR_TABS, inspectorTabIcon, inspectorTabTitle, useApp, type InspectorTab } from '../../state/app'
import { useBrowserDriven } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { AgentsTab } from './AgentsTab'
import { BrowserTab } from './BrowserTab'
import { FilesTab } from './FilesTab'
import { RunTab } from './RunTab'
import { TerminalTab } from './TerminalTab'
import { InspectorPaneActiveContext, cx } from './inspectorShared'
import styles from './InspectorView.module.css'

/** One tab's pane: the shown one, or the one sliding out after a switch. */
interface Pane {
  tab: InspectorTab
  leaving: boolean
}

/** How long a leaving pane may stay when its animation never ends (hidden window). */
const LEAVE_FALLBACK_MS = 600

/**
 * Port of `InspectorView` (Views/Inspector/InspectorView.swift): the right
 * pane. A 52 px header with the icon tab switcher (its selection card slides
 * to the new tab) and the close button, over the selected tab: *Run* (the
 * Backend card, the selected chat's task, context stats, the project's
 * changes and counters), *Files* (the project tree), *Terminal* (shells in
 * the project root), *Agents* (one tab per sub-agent of the chat) and
 * *Browser* (the project's page, which the agent drives without the
 * Inspector switching to it). The tab content slides with the switcher:
 * Run from the leading edge, Browser from the trailing edge, the others
 * fade. The pane's width, hairline and resize handle belong to MainWindow.
 */
export function InspectorView() {
  const tab = useApp((state) => state.inspectorTab)
  const [shown, setShown] = useState<InspectorTab>(tab)
  const [leaving, setLeaving] = useState<InspectorTab | null>(null)
  const [switched, setSwitched] = useState(false)

  // A new tab: the previous one leaves while the new one comes in (the
  // ZStack's insertion and removal transitions in Swift).
  if (shown !== tab) {
    setShown(tab)
    setLeaving(prefersReducedMotion() ? null : shown)
    setSwitched(true)
  }

  useEffect(() => {
    if (leaving === null) return
    const timer = window.setTimeout(() => setLeaving(null), LEAVE_FALLBACK_MS)
    return () => window.clearTimeout(timer)
  }, [leaving])

  const panes: Pane[] = leaving !== null && leaving !== shown ? [{ tab: leaving, leaving: true }, { tab: shown, leaving: false }] : [{ tab: shown, leaving: false }]

  const onLeft = (event: AnimationEvent<HTMLDivElement>, pane: InspectorTab) => {
    if (event.target !== event.currentTarget) return
    setLeaving((current) => (current === pane ? null : current))
  }

  return (
    <aside className={styles.inspector} aria-label="Inspector">
      <div className={styles.header}>
        <InspectorTabs selected={tab} />
        <div className={styles.spacer} />
        <IconButton icon="close" label="Hide inspector" size={12} className={styles.closeButton} onClick={() => useApp.getState().setInspectorVisible(false)} />
      </div>
      <div className={styles.content}>
        {panes.map((pane) => (
          <div
            key={pane.tab}
            className={cx(styles.pane, pane.leaving ? cx(styles.leaving, exitClass(pane.tab)) : switched && enterClass(pane.tab))}
            onAnimationEnd={pane.leaving ? (event) => onLeft(event, pane.tab) : undefined}
            aria-hidden={pane.leaving || undefined}
            inert={pane.leaving || undefined}
          >
            <InspectorPaneActiveContext.Provider value={!pane.leaving}>{tabContent(pane.tab)}</InspectorPaneActiveContext.Provider>
          </div>
        ))}
      </div>
    </aside>
  )
}

function tabContent(tab: InspectorTab): ReactNode {
  switch (tab) {
    case 'run':
      return <RunTab />
    case 'files':
      return <FilesTab />
    case 'terminal':
      return <TerminalTab />
    case 'agents':
      return <AgentsTab />
    case 'browser':
      return <BrowserTab />
  }
}

/** `.transition(.move(edge: .leading) …)` for Run, `.trailing` for Browser, `.opacity` otherwise. */
function enterClass(tab: InspectorTab): string | undefined {
  if (tab === 'run') return styles.enterLeading
  if (tab === 'browser') return styles.enterTrailing
  return styles.enterFade
}

function exitClass(tab: InspectorTab): string | undefined {
  if (tab === 'run') return styles.exitLeading
  if (tab === 'browser') return styles.exitTrailing
  return styles.exitFade
}

// MARK: - Tabs

/**
 * `InspectorTabs`: icon buttons over a raised track; the selected button's
 * card slides to the new tab (`matchedGeometryEffect`), the others lift on
 * hover. The Browser button carries an accent dot while the agent drives
 * the page and another tab is selected.
 */
function InspectorTabs({ selected }: { selected: InspectorTab }) {
  const browserDriven = useBrowserDriven()
  const index = Math.max(0, INSPECTOR_TABS.indexOf(selected))
  return (
    <div className={styles.tabs} role="tablist" aria-label="Inspector tabs">
      <div className={styles.selection} style={{ transform: `translateX(${index * 36}px)` }} aria-hidden="true" />
      {INSPECTOR_TABS.map((tab) => (
        <InspectorTabButton key={tab} tab={tab} selected={tab === selected} busy={tab === 'browser' && browserDriven} />
      ))}
    </div>
  )
}

/** `InspectorTabButton`: one 34 × 24 icon button of the switcher. */
function InspectorTabButton({ tab, selected, busy }: { tab: InspectorTab; selected: boolean; busy: boolean }) {
  const title = inspectorTabTitle(tab)
  const marked = busy && !selected
  return (
    <Tooltip content={marked ? `${title} · the agent is using it` : title}>
      <button
        type="button"
        role="tab"
        aria-selected={selected}
        aria-label={marked ? `${title}, in use by the agent` : title}
        className={cx(styles.tabButton, selected && styles.selected)}
        onClick={() => useApp.getState().setInspectorTab(tab)}
      >
        {/* SF Symbols at 13 pt draw ~15 pt wide glyphs; the Icon's optical size lands there at 13.5. */}
        <Icon name={inspectorTabIcon(tab)} size={13.5} strokeWidth={selected ? 2.25 : 1.75} className={styles.tabIcon} />
        <span className={cx(styles.busyDot, marked && styles.busyDotShown)} aria-hidden="true" />
      </button>
    </Tooltip>
  )
}
