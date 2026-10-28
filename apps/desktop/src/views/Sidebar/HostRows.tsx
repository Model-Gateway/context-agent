// Port of apps/macos/ContextAgent/Views/Sidebar/HostRows.swift: the
// sidebar's "Remote" section: every configured SSH host as a row with its
// connection state, followed by the projects open on it (nested like the
// local ones, worktrees under their parent), its folders still opening and
// its pinned recents that are not open. Hidden while no host is configured.

import { memo, useMemo, useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { ContextMenu } from '../../components/ContextMenu'
import { IconButton } from '../../components/IconButton'
import type { MenuItem } from '../../components/Menu'
import { Sheet } from '../../components/Sheet'
import { Tooltip } from '../../components/Tooltip'
import { pinnedRecentProjects, recentProjectId, type RecentProject } from '../../models/recentProject'
import { usePref } from '../../models/preferences'
import { decodeRemoteProject, hostDisplayName, hostTarget, useSSHHosts, type SSHHost } from '../../models/sshHost'
import { lastPathComponent } from '../../models/recentProject'
import { remoteConnection, SETTINGS_TAB_REMOTE_HOSTS, useApp, type AppStore } from '../../state/app'
import type { ProjectState } from '../../state/project'
import type { RemoteConnection } from '../../state/remote'
import { hiddenWorktrees, isCollapsed, parentProject, pendingProjectPathsOn, projectsOn, topLevelProjects, visibleWorktrees, worktreesOf } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { OpeningProjectRow, PinnedRecentRow, ProjectRow, SidebarSectionHeader, WorktreeRow } from './SidebarRows'
import rows from './SidebarRows.module.css'
import sidebar from './SidebarView.module.css'
import styles from './HostRows.module.css'

/// Everything the Remote section draws, as one string: the section
/// re-renders only when it changes, not on every streamed event.
function remoteSignature(state: AppStore, hosts: readonly SSHHost[]): string {
  const parts: string[] = [state.selectedProjectId ?? '']
  for (const host of hosts) {
    const connection = remoteConnection(host.id)
    parts.push(`h:${host.id}:${hostDisplayName(host)}:${connection?.state.kind ?? 'none'}:${connection?.version ?? 0}`)
    for (const project of topLevelProjects(state, host.id)) {
      const hidden = hiddenWorktrees(state, project)
      parts.push(
        `p:${project.id}:${project.name}:${project.branch ?? ''}:${project.isRunning}:${project.unseenCount}:${worktreesOf(state, project).length}:${isCollapsed(project)}`,
        hidden.map((worktree) => `${worktree.isRunning}:${worktree.unseenCount}`).join(','),
      )
      for (const worktree of visibleWorktrees(state, project)) {
        const parent = parentProject(state, worktree)
        parts.push(`w:${worktree.id}:${worktree.worktreeLabel}:${worktree.branch ?? ''}:${worktree.isRunning}:${worktree.dirtyCount}:${worktree.unseenCount}:${parent?.branch ?? ''}:${parent?.isRunning ?? false}`)
      }
    }
    for (const key of pendingProjectPathsOn(state, host.id)) parts.push(`o:${key}`)
  }
  return parts.join('|')
}

/** `RemoteHostsSection`: the "Remote" header, then each host with its rows beneath it. */
export function RemoteHostsSection() {
  const hosts = useSSHHosts()
  const recents = usePref('recents')
  const signature = useApp((state) => remoteSignature(state, hosts))
  const [headerHovering, setHeaderHovering] = useState(false)

  // `signature` stands for everything read from the store below.
  const content = useMemo(() => {
    const state = useApp.getState()
    const open = new Set([...state.projects.map((project) => project.key), ...state.openingProjectPaths])
    const pinned = pinnedRecentProjects(recents).filter((recent) => recent.hostId !== null && !open.has(recentProjectId(recent)))
    return { state, pinned }
  }, [signature, recents])

  if (hosts.length === 0) return null
  const { state, pinned } = content
  return (
    <>
      <SidebarSectionHeader
        title="Remote"
        onHoverChange={setHeaderHovering}
        trailing={
          <span className={[sidebar.headerButton, headerHovering ? sidebar.headerButtonShown : ''].filter(Boolean).join(' ')}>
            <IconButton icon="newChat" label="Open a project on a remote host…" size={11} onClick={() => useApp.getState().chooseRemoteProject()} />
          </span>
        }
      />
      {hosts.map((host) => (
        <HostGroup key={host.id} host={host} state={state} recents={recents} pinned={pinned.filter((recent) => recent.hostId === host.id)} />
      ))}
    </>
  )
}

/** One host row and everything nested under it. */
function HostGroup({ host, state, recents, pinned }: { host: SSHHost; state: AppStore; recents: readonly RecentProject[]; pinned: RecentProject[] }) {
  const connection = remoteConnection(host.id)
  const ready = connection?.isReady ?? false
  return (
    <>
      <HostRow host={host} connection={connection} version={connection?.version ?? 0} openCount={projectsOn(state, host.id).length} />
      {topLevelProjects(state, host.id).map((project) => (
        <RemoteProjectWithWorktrees key={project.id} project={project} state={state} recentsPinned={recents.some((recent) => recent.hostId === host.id && recent.path === project.root && recent.pinned)} />
      ))}
      {pendingProjectPathsOn(state, host.id).map((key) => {
        const path = decodeRemoteProject(key)?.path ?? key
        const recent = recents.find((candidate) => recentProjectId(candidate) === key)
        return (
          <div key={`opening:${key}`} className={styles.nested}>
            <OpeningProjectRow path={path} name={recent?.name ?? lastPathComponent(path)} />
          </div>
        )
      })}
      {pinned.map((recent) => (
        <div key={recentProjectId(recent)} className={styles.nested}>
          <PinnedRecentRow recent={recent} ready={ready} />
        </div>
      ))}
    </>
  )
}

/** A remote project row (indented under its host) and its worktree rows. */
function RemoteProjectWithWorktrees({ project, state, recentsPinned }: { project: ProjectState; state: AppStore; recentsPinned: boolean }) {
  const isSelected = state.selectedProjectId === project.id
  const hidden = hiddenWorktrees(state, project)
  const showsSpinner = (project.isRunning && !isSelected) || hidden.some((worktree) => worktree.isRunning)
  const unseenCount = project.unseenCount + hidden.reduce((sum, worktree) => sum + worktree.unseenCount, 0)
  return (
    <>
      <div className={styles.nested}>
        <ProjectRow
          project={project}
          name={project.name}
          branch={project.branch && project.branch.length > 0 ? project.branch : null}
          isSelected={isSelected}
          isPinned={recentsPinned}
          hasWorktrees={worktreesOf(state, project).length > 0}
          isCollapsed={isCollapsed(project)}
          showsSpinner={showsSpinner}
          unseenCount={unseenCount}
        />
      </div>
      {visibleWorktrees(state, project).map((worktree) => {
        const parent = parentProject(state, worktree)
        return (
          <div key={worktree.id} className={styles.nested}>
            <WorktreeRow
              project={worktree}
              label={worktree.worktreeLabel}
              branch={worktree.branch}
              root={worktree.root}
              isSelected={state.selectedProjectId === worktree.id}
              isRunning={worktree.isRunning}
              dirtyCount={worktree.dirtyCount}
              unseenCount={worktree.unseenCount}
              parentBranch={parent?.branch ?? parent?.name ?? 'parent'}
              canMerge={!worktree.isRunning && !(parent?.isRunning ?? false)}
            />
          </div>
        )
      })}
    </>
  )
}

// MARK: - HostRow

/** The dot's tint for a connection state. */
function statusTint(connection: RemoteConnection | null): 'success' | 'danger' | 'warning' | 'tertiary' {
  switch (connection?.state.kind) {
    case 'ready':
      return 'success'
    case 'failed':
      return 'danger'
    case 'connecting':
      return 'warning'
    default:
      return 'tertiary'
  }
}

/** The tooltip's second line: the state, with the hint under a failure. */
function statusText(connection: RemoteConnection | null): string {
  if (!connection) return 'Not connected'
  if (connection.state.kind === 'failed' && connection.hint !== null) return `${connection.label}\n${connection.hint}`
  return connection.statusText
}

/**
 * `HostRow`: one remote host: its name with the connection state where the
 * badge sits (spinner while connecting, a dot otherwise); a click opens the
 * folder picker on it; the context menu connects, reconnects, disconnects
 * or removes it.
 */
export const HostRow = memo(function HostRow({ host, connection, openCount }: { host: SSHHost; connection: RemoteConnection | null; openCount: number; /** The connection's version: the memo key for its state (the object is mutated in place). */ version: number }) {
  const [hovering, setHovering] = useState(false)
  const [confirmRemove, setConfirmRemove] = useState(false)
  const name = hostDisplayName(host)
  const isConnecting = connection?.state.kind === 'connecting'
  const isReady = connection?.isReady ?? false
  const anyRunning = useApp((state) => projectsOn(state, host.id).some((project) => project.isRunning))
  const choose = () => useApp.getState().chooseRemoteProject(host)

  const menu = (): MenuItem[] => {
    const items: MenuItem[] = [{ label: `Open Project on ${name}…`, onSelect: choose }]
    if (isReady) {
      items.push({ label: 'Disconnect', disabled: anyRunning, help: anyRunning ? 'Finish or cancel the running tasks first' : undefined, onSelect: () => void useApp.getState().disconnectHost(host) })
    } else {
      items.push({ label: connection?.failure ? 'Reconnect' : 'Connect', disabled: isConnecting, onSelect: () => void useApp.getState().reconnectHost(host) })
    }
    items.push(
      { type: 'separator' },
      { label: 'Edit in Settings…', onSelect: () => useApp.getState().openSettings(SETTINGS_TAB_REMOTE_HOSTS) },
      { label: 'Copy ssh Command', onSelect: () => useApp.getState().copyToPasteboard(`ssh ${hostTarget(host)}`) },
      { type: 'separator' },
      { label: 'Remove Host…', destructive: true, onSelect: () => setConfirmRemove(true) },
    )
    return items
  }

  const tint = statusTint(connection)
  return (
    <>
      <ContextMenu items={menu}>
        <Tooltip content={`${hostTarget(host)}\n${statusText(connection)}`}>
          <button
            type="button"
            className={[rows.row, hovering ? rows.highlighted : ''].filter(Boolean).join(' ')}
            aria-label={`Remote host ${name}, ${connection?.label ?? 'not connected'}`}
            onClick={choose}
            onPointerEnter={() => setHovering(true)}
            onPointerLeave={() => setHovering(false)}
          >
            <span className={[rows.rowIcon, rows.secondary].join(' ')} aria-hidden="true">
              <Icon name="remoteHost" size={15} strokeWidth={1.85} />
            </span>
            <span className={rows.title}>{name}</span>
            <span className={rows.spacer} />
            {isConnecting ? (
              <BrailleSpinner size={11} tint="var(--text-tertiary)" />
            ) : hovering ? (
              <span className={styles.plus} aria-hidden="true">
                <Icon name="newChat" size={11} strokeWidth={2.25} />
              </span>
            ) : (
              <span className={[styles.statusDot, styles[tint]].join(' ')} aria-hidden="true" />
            )}
          </button>
        </Tooltip>
      </ContextMenu>
      <Sheet
        open={confirmRemove}
        onClose={() => setConfirmRemove(false)}
        title={`Remove “${name}”?`}
        width={440}
        footer={
          <>
            <Button variant="bordered" onClick={() => setConfirmRemove(false)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setConfirmRemove(false)
                void useApp.getState().removeHost(host)
              }}
            >
              Remove
            </Button>
          </>
        }
      >
        <p className={rows.sheetMessage}>
          {openCount > 0
            ? `Its ${openCount} open project${openCount === 1 ? '' : 's'} close${openCount === 1 ? 's' : ''} and its recents are forgotten. Nothing on the host is touched.`
            : 'Its recents are forgotten. Nothing on the host is touched.'}
        </p>
      </Sheet>
    </>
  )
})
