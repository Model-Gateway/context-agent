import { useMemo, useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { IconButton } from '../../components/IconButton'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { Tooltip } from '../../components/Tooltip'
import { usePref } from '../../models/preferences'
import { lastPathComponent, pinnedRecentProjects, RecentProjects } from '../../models/recentProject'
import type { Unseen } from '../../models/reducer'
import { restoreRank, useApp, type AppStore } from '../../state/app'
import type { ChatState } from '../../state/chat'
import type { ProjectState } from '../../state/project'
import { hiddenWorktrees, isCollapsed, parentProject, pendingProjectPaths, selectedProject, topLevelProjects, visibleWorktrees, worktreesOf } from '../../state/selectors'
import { useGateway } from '../../state/gateway'
import { Icon } from '../../theme/icons'
import { NavRow, OpeningProjectRow, PinnedRecentRow, ProjectRow, SessionRow, SessionRowsSkeleton, SidebarSectionHeader, WorktreeRow, type SessionIndicator } from './SidebarRows'
import styles from './SidebarView.module.css'

/**
 * `SidebarView` (Views/Sidebar/SidebarView.swift): the left pane under the
 * top bar: the fixed navigation rows, the "Projects" section (open projects
 * with their worktrees, folders still opening, then pinned recents that are
 * not open), the "Chats" section (the selected project's chats) and the
 * account button at the bottom.
 */
export function SidebarView() {
  return (
    <nav className={styles.sidebar} aria-label="Sidebar">
      <Navigation />
      <div className={styles.scroll}>
        <div className={styles.sections}>
          <ProjectsSection />
          <ChatsSection />
        </div>
      </div>
      <div className={styles.accountSlot}>
        <AccountButton />
      </div>
    </nav>
  )
}

// MARK: - Navigation

/** Welcome · New chat · Projects · Apps (Changes and Project live in the top bar's tabs only). */
function Navigation() {
  const sidebarItem = useApp((state) => state.sidebarItem)
  const hasProject = useApp((state) => selectedProject(state) !== null)
  const newChatSelected = useApp((state) => state.sidebarItem === 'chat' && state.selectedSessionId === null)
  return (
    <div className={styles.navigation}>
      <NavRow icon="welcome" title="Welcome" isSelected={sidebarItem === 'welcome'} onSelect={selectWelcome} />
      <NavRow icon="newChat" title="New chat" isSelected={newChatSelected && hasProject} isEnabled={hasProject} onSelect={newChat} />
      <NavRow icon="projects" title="Projects" isSelected={sidebarItem === 'projects'} onSelect={selectProjects} />
      <NavRow icon="apps" title="Apps" isSelected={sidebarItem === 'apps'} onSelect={selectApps} />
    </div>
  )
}

function selectWelcome() {
  useApp.getState().setSidebarItem('welcome')
}

function newChat() {
  useApp.getState().newChat()
}

function selectProjects() {
  useApp.getState().setSidebarItem('projects')
}

function selectApps() {
  useApp.getState().setSidebarItem('apps')
}

// MARK: - Projects

/** An open project with the worktree rows under it, or a folder still opening. */
type ProjectEntry = { kind: 'open'; project: ProjectState } | { kind: 'opening'; path: string }

/// Open projects with the folders still opening slotted in at their
/// remembered launch position (new opens go last), so rows fill in place.
function projectEntries(state: AppStore): ProjectEntry[] {
  const entries: ProjectEntry[] = topLevelProjects(state).map((project) => ({ kind: 'open', project }))
  for (const path of pendingProjectPaths(state)) {
    const rank = restoreRank(path)
    const index = entries.findIndex((entry) => restoreRank(entry.kind === 'open' ? entry.project.root : entry.path) > rank)
    entries.splice(index < 0 ? entries.length : index, 0, { kind: 'opening', path })
  }
  return entries
}

/// Everything the Projects section draws, as one string: the section
/// re-renders only when it changes, not on every streamed event.
function projectsSignature(state: AppStore): string {
  const parts: string[] = [state.selectedProjectId ?? '', state.connection.kind]
  for (const entry of projectEntries(state)) {
    if (entry.kind === 'opening') {
      parts.push(`o:${entry.path}`)
      continue
    }
    const project = entry.project
    const hidden = hiddenWorktrees(state, project)
    parts.push(
      `p:${project.id}:${project.name}:${project.branch ?? ''}:${project.isRunning}:${project.unseenCount}:${worktreesOf(state, project).length}:${isCollapsed(project)}`,
      hidden.map((worktree) => `${worktree.isRunning}:${worktree.unseenCount}`).join(','),
    )
    for (const worktree of visibleWorktrees(state, project)) {
      const parent = parentProject(state, worktree)
      parts.push(
        `w:${worktree.id}:${worktree.worktreeLabel}:${worktree.branch ?? ''}:${worktree.root}:${worktree.isRunning}:${worktree.dirtyCount}:${worktree.unseenCount}:${parent?.branch ?? parent?.name ?? ''}:${parent?.isRunning ?? false}`,
      )
    }
  }
  return parts.join('|')
}

function ProjectsSection() {
  const signature = useApp(projectsSignature)
  const recents = usePref('recents')
  const [headerHovering, setHeaderHovering] = useState(false)
  const ready = useApp((state) => state.connection.kind === 'ready')

  // `signature` stands for everything read from the store below.
  const content = useMemo(() => {
    const state = useApp.getState()
    const entries = projectEntries(state)
    const open = new Set([...state.projects.map((project) => project.root), ...state.openingProjectPaths])
    const pinned = pinnedRecentProjects(recents).filter((recent) => !open.has(recent.path))
    return { entries, pinned, state }
  }, [signature, recents])

  const { entries, pinned, state } = content
  if (entries.length === 0 && pinned.length === 0) return null
  return (
    <>
      <SidebarSectionHeader
        title="Projects"
        onHoverChange={setHeaderHovering}
        trailing={
          <span className={[styles.headerButton, headerHovering ? styles.headerButtonShown : ''].filter(Boolean).join(' ')}>
            <IconButton icon="newChat" label="Open a project…" size={11} disabled={!ready} onClick={() => void useApp.getState().chooseProject()} />
          </span>
        }
      />
      {entries.map((entry) =>
        entry.kind === 'opening' ? (
          <OpeningProjectRow key={`opening:${entry.path}`} path={entry.path} name={RecentProjects.item(entry.path)?.name ?? lastPathComponent(entry.path)} />
        ) : (
          <ProjectWithWorktrees key={entry.project.id} project={entry.project} state={state} recentsPinned={isPinned(recents, entry.project.root)} />
        ),
      )}
      {pinned.map((recent) => (
        <PinnedRecentRow key={recent.path} recent={recent} ready={ready} />
      ))}
    </>
  )
}

function isPinned(recents: ReturnType<typeof RecentProjects.items>, root: string): boolean {
  return recents.find((recent) => recent.path === root)?.pinned === true
}

/** One project row and the worktree rows nested under it. */
function ProjectWithWorktrees({ project, state, recentsPinned }: { project: ProjectState; state: AppStore; recentsPinned: boolean }) {
  const isSelected = state.selectedProjectId === project.id
  const hidden = hiddenWorktrees(state, project)
  const showsSpinner = (project.isRunning && !isSelected) || hidden.some((worktree) => worktree.isRunning)
  const unseenCount = project.unseenCount + hidden.reduce((sum, worktree) => sum + worktree.unseenCount, 0)
  return (
    <>
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
      {visibleWorktrees(state, project).map((worktree) => {
        const parent = parentProject(state, worktree)
        return (
          <WorktreeRow
            key={worktree.id}
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
        )
      })}
    </>
  )
}

// MARK: - Chats

/** The dot / spinner a chat row shows (SessionRow's `needsHelp`, `isRunning`, `unseen`). */
function sessionIndicator(chat: ChatState | null, isRunning: boolean): SessionIndicator {
  const unseen = chat?.unseen ?? null
  if (chat?.needsHelp) return { kind: 'help', label: helpLabel(chat, unseen) }
  if (isRunning) return { kind: 'running' }
  if (unseen) return { kind: 'unseen', tint: dotTint(unseen), label: dotLabel(unseen) }
  return null
}

/** Why the orange dot is up. */
function helpLabel(chat: ChatState, unseen: Unseen | null): string {
  if (chat.pendingQuestion !== null) return 'needs your answer'
  if (chat.pendingApproval !== null || unseen?.kind === 'approval') return 'approval waiting'
  return 'blocked, needs your help'
}

function dotTint(unseen: Unseen): 'accent' | 'danger' | 'warning' {
  switch (unseen.kind) {
    case 'finished':
      if (unseen.status === 'failed') return 'danger'
      if (unseen.status === 'blocked') return 'warning'
      return 'accent'
    case 'approval':
      return 'warning'
    case 'error':
      return 'danger'
  }
}

function dotLabel(unseen: Unseen): string {
  switch (unseen.kind) {
    case 'finished':
      return unseen.status === 'completed' ? 'finished, not opened yet' : `task ${unseen.status}, not opened yet`
    case 'approval':
      return 'approval waiting'
    case 'error':
      return 'error, not opened yet'
  }
}

function indicatorKey(indicator: SessionIndicator): string {
  if (indicator === null) return ''
  if (indicator.kind === 'running') return 'r'
  if (indicator.kind === 'help') return `h:${indicator.label}`
  return `u:${indicator.tint}:${indicator.label}`
}

/// Everything the Chats section draws, as one string (see `projectsSignature`).
function chatsSignature(state: AppStore): string {
  const project = selectedProject(state)
  if (!project) return `none:${pendingProjectPaths(state).length > 0}`
  const parts = [project.id, state.selectedSessionId ?? '', `${project.sessionsLoading}`, project.sessionsError ?? '']
  for (const session of project.sessions) {
    const running = project.isRunningSession(session.id)
    parts.push(`${session.id}:${session.title}:${session.updated_at}:${running}:${indicatorKey(sessionIndicator(project.existingChat(session.id), running))}`)
  }
  return parts.join('|')
}

function ChatsSection() {
  const signature = useApp(chatsSignature)
  const [headerHovering, setHeaderHovering] = useState(false)

  // `signature` stands for everything read from the store below.
  const content = useMemo(() => {
    const state = useApp.getState()
    const project = selectedProject(state)
    return { project, selectedSessionId: state.selectedSessionId, pending: pendingProjectPaths(state).length > 0 }
  }, [signature])

  const { project, selectedSessionId, pending } = content
  if (!project) {
    if (!pending) return null
    // The project about to be selected is still opening.
    return (
      <>
        <SidebarSectionHeader title="Chats" />
        <SessionRowsSkeleton />
      </>
    )
  }

  const sessions = project.sessions
  return (
    <>
      <SidebarSectionHeader
        title="Chats"
        onHoverChange={setHeaderHovering}
        trailing={
          // The button always holds its space so the header never changes
          // height; it fades in while the header is hovered (the spinner
          // takes its place otherwise).
          <span className={styles.chatsHeaderTrailing}>
            {!headerHovering && project.sessionsLoading && sessions.length > 0 ? (
              <span className={styles.headerSpinner}>
                <BrailleSpinner size={10} tint="var(--text-tertiary)" />
              </span>
            ) : null}
            <span className={[styles.headerButton, headerHovering ? styles.headerButtonShown : ''].filter(Boolean).join(' ')}>
              <IconButton icon="newChat" label="New chat" size={11} onClick={newChat} />
            </span>
          </span>
        }
      />
      {sessions.length === 0 ? (
        project.sessionsLoading ? (
          <SessionRowsSkeleton />
        ) : project.sessionsError !== null ? (
          <SessionsError error={project.sessionsError} project={project} />
        ) : (
          <div className={styles.noChats}>No chats yet</div>
        )
      ) : (
        <>
          {sessions.map((session) => {
            const running = project.isRunningSession(session.id)
            return (
              <SessionRow
                key={session.id}
                session={session}
                isSelected={selectedSessionId === session.id}
                isRunning={running}
                indicator={sessionIndicator(project.existingChat(session.id), running)}
              />
            )
          })}
          {project.sessionsError !== null ? <SessionsError error={project.sessionsError} project={project} /> : null}
        </>
      )}
    </>
  )
}

/** The chat list could not be loaded: the error and a small Retry. */
function SessionsError({ error, project }: { error: string; project: ProjectState }) {
  return (
    <div className={styles.sessionsError}>
      <p className={`${styles.sessionsErrorText} selectable`}>{error}</p>
      <Button variant="bordered" size="small" className={styles.retry} onClick={() => void project.loadSessions()}>
        Retry
      </Button>
    </div>
  )
}

// MARK: - Account button

/// `AccountButton`: the bottom-left "Sign in with ModelGateway · Optional"
/// (ARCHITECTURE.md "Account"): opens the profile page (Account and the
/// settings tabs) on its last tab; signed out and landing on Account, it
/// also starts the device sign-in. Signed in it shows the account's
/// initial, name and balance.
function AccountButton() {
  const selected = useApp((state) => state.sidebarItem === 'account')
  const title = useGateway((state) => state.title)
  const subtitle = useGateway((state) => state.subtitle)
  const help = useGateway((state) => state.help)
  const initial = useGateway((state) => state.initial)
  const signingIn = useGateway((state) => state.isSigningIn)
  const [hovering, setHovering] = useState(false)
  return (
    <Tooltip content={help}>
      <button
        type="button"
        className={[styles.account, selected || hovering ? styles.accountHighlighted : ''].filter(Boolean).join(' ')}
        aria-label={`${title}. ${subtitle}`}
        aria-current={selected ? 'page' : undefined}
        onClick={() => useGateway.getState().openAccount()}
        onPointerEnter={() => setHovering(true)}
        onPointerLeave={() => setHovering(false)}
      >
        {initial !== null ? (
          <span className={styles.avatar} aria-hidden="true">
            {initial}
          </span>
        ) : (
          <span className={styles.avatarIcon} aria-hidden="true">
            <Icon name="account" size={17.5} strokeWidth={1.6} />
          </span>
        )}
        <span className={styles.accountText}>
          <span className={styles.accountTitle}>{title}</span>
          <MiddleTruncate text={subtitle} className={styles.accountSubtitle} />
        </span>
        {signingIn ? (
          <BrailleSpinner size={10} tint="var(--text-secondary)" />
        ) : (
          <Icon name="chevronRight" size={11.5} strokeWidth={2.5} className={styles.accountChevron} />
        )}
      </button>
    </Tooltip>
  )
}
