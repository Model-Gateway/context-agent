import { useMemo } from 'react'
import { describe } from '../../backend/errors'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { Tooltip } from '../../components/Tooltip'
import { abbreviatedPath, useRecentProjects } from '../../models/recentProject'
import { useApp, type ConnectionState } from '../../state/app'
import { useConnectionLabel, useNeedsAPIKey } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { LogoView } from '../../theme/logo'
import { BackendFailurePanel } from './BackendFailurePanel'
import { ConnectModelCard } from './ConnectModelCard'
import { RecentCard } from './RecentCard'
import styles from './WelcomeView.module.css'

/**
 * `WelcomeView` (Views/Welcome/WelcomeView.swift): the hero ("Context",
 * Open a project…), the backend failure panel or the Connect-a-model card
 * when needed, the recent projects grid and the backend status footer. It
 * stays usable while the backend is down: the recents remain and the open
 * button says why it is disabled. Folders dropped on it open as projects
 * (MainWindow owns the drop target and its highlight).
 */
export function WelcomeView() {
  const connection = useApp((state) => state.connection)
  const needsAPIKey = useNeedsAPIKey()
  return (
    <div className={styles.scroll} role="region" aria-label="Welcome">
      <div className={styles.column}>
        <Hero connection={connection} />
        {connection.kind === 'failed' ? <BackendFailurePanel failure={connection.failure} /> : null}
        {needsAPIKey && connection.kind === 'ready' ? <ConnectModelCard /> : null}
        <RecentsGrid />
        <Footer connection={connection} />
      </div>
    </div>
  )
}

// MARK: - Hero

/** Why "Open a project…" is disabled, or null when it is not. */
function disabledReason(connection: ConnectionState): string | null {
  switch (connection.kind) {
    case 'connecting':
      return 'Starting the backend…'
    case 'ready':
      return null
    case 'failed':
      return `${describe(connection.failure).title} — fix the backend to open a project`
  }
}

function Hero({ connection }: { connection: ConnectionState }) {
  const reason = disabledReason(connection)
  return (
    <div className={styles.hero}>
      <LogoView size={34} />
      <h1 className={styles.heroTitle}>Context</h1>
      <div className={styles.openBlock}>
        <Tooltip content={reason ?? 'Choose a repository folder'} shortcut={reason === null ? 'mod+O' : undefined}>
          <button type="button" className={styles.openButton} disabled={connection.kind !== 'ready'} onClick={() => void useApp.getState().chooseProject()}>
            {connection.kind === 'connecting' ? <BrailleSpinner size={13} tint="inherit" /> : <Icon name="projects" size={14} strokeWidth={2.1} />}
            <span>Open a project…</span>
          </button>
        </Tooltip>
        <p className={styles.openHint}>{reason ?? 'or drop a folder here'}</p>
      </div>
    </div>
  )
}

// MARK: - Recents grid

/// The recent projects as cards, pinned first; "View all" opens the
/// Projects screen (search, filter, sort).
function RecentsGrid() {
  const recents = useRecentProjects()
  const projects = useApp((state) => state.projects)
  const openRoots = useMemo(() => new Set(projects.map((project) => project.root)), [projects])
  return (
    <section className={styles.recents} aria-labelledby="welcome-recents-title">
      <div className={styles.recentsHeader}>
        <h2 id="welcome-recents-title" className={styles.recentsTitle}>
          Recent projects
        </h2>
        <span className={styles.spacer} />
        {recents.length > 0 ? (
          <>
            <span className={styles.count}>{recents.length}</span>
            <Tooltip content="Search and filter all projects">
              <button type="button" className={styles.viewAll} onClick={() => useApp.getState().setSidebarItem('projects')}>
                View all
              </button>
            </Tooltip>
          </>
        ) : null}
      </div>
      {recents.length === 0 ? (
        <p className={styles.emptyRecents}>Projects you open show up here.</p>
      ) : (
        <div className={styles.grid}>
          {recents.map((recent) => (
            <RecentCard key={recent.path} recent={recent} isOpen={openRoots.has(recent.path)} />
          ))}
        </div>
      )}
    </section>
  )
}

// MARK: - Footer

function Footer({ connection }: { connection: ConnectionState }) {
  const label = useConnectionLabel()
  const dataDir = useApp((state) => state.config?.data_dir ?? '')
  return (
    <footer className={styles.footer}>
      {connection.kind === 'connecting' ? (
        <BrailleSpinner size={10} tint="var(--text-tertiary)" />
      ) : (
        <span className={[styles.dot, connection.kind === 'ready' ? styles.dotReady : styles.dotFailed].join(' ')} aria-hidden="true" />
      )}
      <span className={styles.footerLabel}>{label}</span>
      {dataDir.length > 0 ? (
        <>
          <span className={styles.footerSeparator} aria-hidden="true">
            ·
          </span>
          <Tooltip content={`Data directory: ${dataDir}`}>
            <span className={styles.dataDirSlot}>
              <MiddleTruncate text={abbreviatedPath(dataDir)} className={styles.dataDir} />
            </span>
          </Tooltip>
        </>
      ) : (
        <span className={styles.spacer} />
      )}
      <Tooltip content="Open Settings" shortcut="mod+,">
        <button type="button" className={styles.settings} onClick={() => useApp.getState().openSettings()}>
          <Icon name="settings" size={11} strokeWidth={2} />
          <span>Settings</span>
        </button>
      </Tooltip>
    </footer>
  )
}
