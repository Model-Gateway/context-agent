// Port of apps/macos/ContextAgent/Views/Sidebar/SidebarRows.swift: the
// sidebar's rows (apps/macos/ARCHITECTURE.md "Sidebar"): 30 px tall, 13.5 px
// text, 16 px glyphs in `textSecondary`, an 8 px `surfaceRaised` background
// on hover and selection. `NavRow` is a fixed item, `ProjectRow` an open
// project, `WorktreeRow` a worktree nested under it, `OpeningProjectRow` a
// folder still opening, `PinnedRecentRow` a pinned recent that is not open
// and `SessionRow` one chat with inline rename and delete confirmation.
//
// Rows take primitives (plus the object their actions need) and are
// memoised, so the sidebar re-renders a row only when what it draws changes.

import { memo, useEffect, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { errorText } from '../../backend/errors'
import { sessionUpdatedDate, worktreeEntryName, type SessionSummary, type WorktreeEntry } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { ContextMenu } from '../../components/ContextMenu'
import { confirm } from '../../components/dialogs'
import { IconButton } from '../../components/IconButton'
import { Kbd } from '../../components/Kbd'
import type { MenuItem } from '../../components/Menu'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { revealLabel } from '../../components/platform'
import { Sheet } from '../../components/Sheet'
import { Skeleton } from '../../components/Skeleton'
import { toasts } from '../../components/toasts'
import { Tooltip } from '../../components/Tooltip'
import { abbreviatedPath, RecentProjects, type RecentProject } from '../../models/recentProject'
import { TranscriptFormat } from '../../models/transcript'
import { useApp } from '../../state/app'
import type { ProjectState } from '../../state/project'
import { mergeWorktree, newWorktree, openWorktree, removeWorktree } from '../../state/worktrees'
import { Icon, type IconName } from '../../theme/icons'
import styles from './SidebarRows.module.css'

// MARK: - Shared row chrome

/** `SidebarRowChrome`: the 30 px row with its 8 px hover / selection background. */
function rowClass(selected: boolean, hovering: boolean, extra?: string): string {
  return [styles.row, selected || hovering ? styles.highlighted : '', extra ?? ''].filter(Boolean).join(' ')
}

/// `RowIcon`: the 16 pt medium symbol in a 22 px box, `textSecondary`
/// (accent on a selected row, tertiary when muted). An SF Symbol at 16 pt
/// draws about 18 px wide; the Icon's optical size lands there at 15.
function RowIcon({ icon, tint = 'secondary' }: { icon: IconName; tint?: 'secondary' | 'accent' | 'tertiary' }) {
  return (
    <span className={[styles.rowIcon, styles[tint]].join(' ')} aria-hidden="true">
      <Icon name={icon} size={15} strokeWidth={1.85} />
    </span>
  )
}

/** Hover state of one row (`onHover` with `.easeOut(duration: 0.12)`, the fade is CSS). */
function useHover(): [boolean, { onPointerEnter: () => void; onPointerLeave: () => void }] {
  const [hovering, setHovering] = useState(false)
  return [hovering, { onPointerEnter: () => setHovering(true), onPointerLeave: () => setHovering(false) }]
}

/** Enter / Space on a `role="button"` row (rows that hold buttons of their own cannot be `<button>`s). */
function activateOnKey(action: () => void) {
  return (event: KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      action()
    }
  }
}

/** "Reveal in Finder" / "Show in Explorer" for a folder. */
function revealFolder(path: string): void {
  void invoke<void>('reveal_in_file_manager', { path }).catch((error: unknown) => {
    toasts.push({ message: `Could not show ${path}: ${errorText(error)}`, level: 'error' })
  })
}

/** Stops a click on a control inside a row from also activating the row. */
function stop(action: () => void) {
  return (event: MouseEvent<HTMLElement>) => {
    event.stopPropagation()
    action()
  }
}

// MARK: - Section header

export interface SidebarSectionHeaderProps {
  title: string
  /** The header's trailing control (it fades in while the header is hovered). */
  trailing?: ReactNode
  onHoverChange?: (hovering: boolean) => void
}

/** `SidebarSectionHeader`: "Projects" / "Chats", 12 px medium `textTertiary`. */
export function SidebarSectionHeader({ title, trailing, onHoverChange }: SidebarSectionHeaderProps) {
  return (
    <div className={styles.sectionHeader} onPointerEnter={() => onHoverChange?.(true)} onPointerLeave={() => onHoverChange?.(false)}>
      <h3 className={styles.sectionTitle}>{title}</h3>
      <span className={styles.flexSpacer} />
      {trailing}
    </div>
  )
}

// MARK: - NavRow

export interface NavRowProps {
  icon: IconName
  title: string
  isSelected?: boolean
  isEnabled?: boolean
  onSelect: () => void
  trailing?: ReactNode
}

/** `NavRow`: Welcome · New chat · Projects · Apps. */
export const NavRow = memo(function NavRow({ icon, title, isSelected = false, isEnabled = true, onSelect, trailing }: NavRowProps) {
  const [hovering, hover] = useHover()
  return (
    <button
      type="button"
      className={rowClass(isSelected, hovering && isEnabled, isEnabled ? '' : styles.disabledRow)}
      disabled={!isEnabled}
      aria-label={title}
      aria-current={isSelected ? 'page' : undefined}
      onClick={onSelect}
      {...hover}
    >
      <RowIcon icon={icon} />
      <span className={styles.title}>{title}</span>
      <span className={styles.spacer} />
      {trailing}
    </button>
  )
})

// MARK: - Trailing slot

/// One trailing slot, always as wide as the close button: hover swaps the
/// spinner / badge for the × without changing the layout, so the name-and-
/// branch arrangement before it never flips.
function TrailingSlot({ hovering, status, closeLabel, onClose }: { hovering: boolean; status: ReactNode; closeLabel: string; onClose: () => void }) {
  return (
    <span className={styles.trailingSlot}>
      <span className={[styles.statusLayer, hovering ? styles.hidden : ''].filter(Boolean).join(' ')}>{status}</span>
      <span className={[styles.closeLayer, hovering ? styles.shown : ''].filter(Boolean).join(' ')} aria-hidden={!hovering || undefined}>
        <IconButton icon="close" label={closeLabel} size={11} tabIndex={hovering ? 0 : -1} onClick={stop(onClose)} />
      </span>
    </span>
  )
}

function RunningSpinner() {
  return (
    <Tooltip content="A task is running">
      <BrailleSpinner size={11} />
    </Tooltip>
  )
}

// MARK: - ProjectRow

export interface ProjectRowProps {
  project: ProjectState
  name: string
  branch: string | null
  isSelected: boolean
  isPinned: boolean
  /** Worktrees are listed below it (the collapse chevron shows). */
  hasWorktrees: boolean
  isCollapsed: boolean
  /** Running and not selected, or a hidden worktree is running. */
  showsSpinner: boolean
  /** Its unseen chats plus those of the worktrees it hides. */
  unseenCount: number
}

/// `ProjectRow`: one open project: folder glyph, name, branch chip, a
/// collapse chevron when worktrees are listed below it, a spinner while it
/// runs unselected, an unseen-chats badge, a close × on hover and a context
/// menu. Collapsed, the hidden worktrees' spinner and badge roll up into it.
export const ProjectRow = memo(function ProjectRow({ project, name, branch, isSelected, isPinned, hasWorktrees, isCollapsed, showsSpinner, unseenCount }: ProjectRowProps) {
  const [hovering, hover] = useHover()

  const toggleCollapsed = () => useApp.getState().setCollapsed(!isCollapsed, project)
  const select = () => useApp.getState().selectProject(project.id)

  const askClose = async () => {
    const running = project.isRunning
    const several = project.runningSessionIds.size > 1
    const confirmed = await confirm({
      title: `Close “${project.name}”?`,
      message: running
        ? `Its running ${several ? 'tasks' : 'task'} will be cancelled. Chats are kept; reopen the project any time from Projects.`
        : 'Chats are kept; reopen the project any time from Projects.',
      confirmLabel: running ? (several ? 'Stop Tasks and Close' : 'Stop Task and Close') : 'Close Project',
      destructive: true,
    })
    if (confirmed) await useApp.getState().closeProject(project.id)
  }

  const menu = (): MenuItem[] => {
    const openRoots = new Set(useApp.getState().projects.map((candidate) => candidate.root))
    const closedWorktrees = project.worktrees.filter((entry) => entry.managed && !openRoots.has(entry.path))
    const items: MenuItem[] = [
      {
        label: 'New Chat in a Worktree',
        disabled: !project.isGitRepository || project.creatingWorktree,
        onSelect: () => void newWorktree(project),
      },
    ]
    // NSMenu's "Open Worktree" submenu, flattened under a header (Menu has no submenus).
    if (closedWorktrees.length > 0) {
      items.push({ type: 'header', label: 'Open Worktree' })
      for (const entry of closedWorktrees) items.push({ id: `worktree:${entry.path}`, label: worktreeEntryLabel(entry), onSelect: () => void openWorktree(entry) })
    }
    if (hasWorktrees) items.push({ label: isCollapsed ? 'Show Worktrees' : 'Hide Worktrees', onSelect: toggleCollapsed })
    items.push(
      { type: 'separator' },
      {
        label: isPinned ? 'Unpin' : 'Pin',
        onSelect: () => {
          if (RecentProjects.item(project.root) === null) RecentProjects.touch(project.root, project.name)
          RecentProjects.togglePin(project.root)
        },
      },
      { label: revealLabel, onSelect: () => revealFolder(project.root) },
      { label: 'Copy Path', onSelect: () => useApp.getState().copyToPasteboard(project.root) },
      { type: 'separator' },
      { label: 'Close…', onSelect: () => void askClose() },
    )
    return items
  }

  return (
    <ContextMenu items={menu}>
      <div
        role="button"
        tabIndex={0}
        className={rowClass(isSelected, hovering)}
        aria-label={`Project ${name}`}
        aria-current={isSelected ? 'true' : undefined}
        onClick={select}
        onKeyDown={activateOnKey(select)}
        {...hover}
      >
        <RowIcon icon="folder" tint={isSelected ? 'accent' : 'secondary'} />
        {/* Name + branch chip when both fit; otherwise the name alone, truncated at the end. */}
        <span className={styles.nameAndBranch}>
          <span className={styles.fitName}>{name}</span>
          {branch ? (
            <Tooltip content={`Branch ${branch}`}>
              <Chip icon="branch" className={styles.fitChip}>
                {branch}
              </Chip>
            </Tooltip>
          ) : null}
        </span>
        {hasWorktrees ? (
          <Tooltip content={isCollapsed ? 'Show worktrees' : 'Hide worktrees'}>
            <button
              type="button"
              className={[styles.collapse, isCollapsed ? '' : styles.expanded].filter(Boolean).join(' ')}
              aria-label={isCollapsed ? 'Show worktrees' : 'Hide worktrees'}
              aria-expanded={!isCollapsed}
              onClick={stop(toggleCollapsed)}
            >
              <Icon name="chevronRight" size={10} strokeWidth={2.75} />
            </button>
          </Tooltip>
        ) : null}
        <span className={styles.spacer} />
        <TrailingSlot
          hovering={hovering}
          closeLabel="Close project"
          onClose={() => void askClose()}
          status={
            <>
              {showsSpinner ? <RunningSpinner /> : null}
              {unseenCount > 0 ? (
                <Tooltip content={`${unseenCount} chat${unseenCount === 1 ? '' : 's'} with news`}>
                  <Chip variant="badge" aria-label={`${unseenCount} unseen chats`}>
                    {unseenCount}
                  </Chip>
                </Tooltip>
              ) : null}
            </>
          }
        />
      </div>
    </ContextMenu>
  )
})

/** A worktree entry's menu title: its branch without the `ctx/` prefix, else its folder name. */
function worktreeEntryLabel(entry: WorktreeEntry): string {
  const branch = entry.branch
  if (branch == null) return worktreeEntryName(entry)
  return branch.startsWith('ctx/') ? branch.slice(4) : branch
}

// MARK: - WorktreeRow

export interface WorktreeRowProps {
  project: ProjectState
  label: string
  branch: string | null
  root: string
  isSelected: boolean
  isRunning: boolean
  dirtyCount: number
  unseenCount: number
  /** The branch a merge lands on (the parent's branch, else its folder name). */
  parentBranch: string
  /** Neither checkout is running a task. */
  canMerge: boolean
}

type WorktreeDialog = 'merge' | 'remove' | null

/// `WorktreeRow`: a worktree of the project above it, indented: branch
/// glyph, the branch name without its `ctx/` prefix, a changed-files chip, a
/// spinner while it runs unselected, the unseen badge, a remove × on hover
/// and a context menu. Removing asks whether the branch goes too.
export const WorktreeRow = memo(function WorktreeRow({ project, label, branch, root, isSelected, isRunning, dirtyCount, unseenCount, parentBranch, canMerge }: WorktreeRowProps) {
  const [hovering, hover] = useHover()
  const [overControl, setOverControl] = useState(false)
  const [dialog, setDialog] = useState<WorktreeDialog>(null)
  const select = () => useApp.getState().selectProject(project.id)
  const changes = `${dirtyCount} uncommitted change${dirtyCount === 1 ? '' : 's'}`

  const menu = (): MenuItem[] => {
    const items: MenuItem[] = [
      { label: revealLabel, onSelect: () => revealFolder(project.root) },
      { label: 'Copy Path', onSelect: () => useApp.getState().copyToPasteboard(project.root) },
    ]
    if (branch !== null) items.push({ label: 'Copy Branch Name', onSelect: () => useApp.getState().copyToPasteboard(branch) })
    items.push(
      { type: 'separator' },
      { label: `Merge into ${parentBranch}…`, disabled: !canMerge, onSelect: () => setDialog('merge') },
      { type: 'separator' },
      { label: 'Close (Keep Worktree)', onSelect: () => void useApp.getState().closeProject(project.id) },
      { label: 'Remove Worktree…', destructive: true, disabled: isRunning, onSelect: () => setDialog('remove') },
    )
    return items
  }

  return (
    <>
      <ContextMenu items={menu}>
        <Tooltip content={`Worktree on ${branch ?? project.name}\n${root}`} disabled={overControl}>
          <div
            role="button"
            tabIndex={0}
            className={rowClass(isSelected, hovering, styles.worktreeRow)}
            aria-label={`Worktree ${label}`}
            aria-current={isSelected ? 'true' : undefined}
            onClick={select}
            onKeyDown={activateOnKey(select)}
            onPointerEnter={hover.onPointerEnter}
            onPointerLeave={() => {
              hover.onPointerLeave()
              setOverControl(false)
            }}
          >
            <RowIcon icon="worktree" tint={isSelected ? 'accent' : 'secondary'} />
            <span className={styles.worktreeLabel}>{label}</span>
            {dirtyCount > 0 ? (
              <span className={styles.chipSlot} onPointerEnter={() => setOverControl(true)} onPointerLeave={() => setOverControl(false)}>
                <Tooltip content={`${dirtyCount} changed file${dirtyCount === 1 ? '' : 's'}`}>
                  <Chip icon="edit">{dirtyCount}</Chip>
                </Tooltip>
              </span>
            ) : null}
            <span className={styles.spacer} />
            <span className={styles.chipSlot} onPointerEnter={() => setOverControl(true)} onPointerLeave={() => setOverControl(false)}>
              <TrailingSlot
                hovering={hovering}
                closeLabel="Remove worktree"
                onClose={() => setDialog('remove')}
                status={
                  <>
                    {isRunning && !isSelected ? <RunningSpinner /> : null}
                    {unseenCount > 0 ? (
                      <Chip variant="badge" aria-label={`${unseenCount} unseen chats`}>
                        {unseenCount}
                      </Chip>
                    ) : null}
                  </>
                }
              />
            </span>
          </div>
        </Tooltip>
      </ContextMenu>
      <ChoiceSheet
        open={dialog === 'merge'}
        title={`Merge “${label}” into ${parentBranch}?`}
        message={
          dirtyCount > 0
            ? `The worktree has ${changes}; commit them in its Changes tab first. The branch is merged into ${parentBranch} with a merge commit; a conflict leaves ${parentBranch} untouched.`
            : `The branch is merged into ${parentBranch} with a merge commit; a conflict leaves ${parentBranch} untouched. “Merge and Remove Worktree” also deletes the checkout and the branch.`
        }
        choices={[
          { label: 'Merge and Remove Worktree', onSelect: () => void mergeWorktree(project, true) },
          { label: 'Merge', primary: true, onSelect: () => void mergeWorktree(project, false) },
        ]}
        onCancel={() => setDialog(null)}
        onDone={() => setDialog(null)}
      />
      <ChoiceSheet
        open={dialog === 'remove'}
        title={`Remove worktree “${label}”?`}
        message={
          dirtyCount > 0
            ? `The checkout and its ${changes} are deleted. Commits on the branch are kept unless the branch is deleted too.`
            : 'The checkout is deleted. Commits on the branch are kept unless the branch is deleted too.'
        }
        choices={[
          { label: 'Remove and Delete Branch', destructive: true, onSelect: () => void removeWorktree(project, true) },
          { label: 'Remove, Keep Branch', primary: true, onSelect: () => void removeWorktree(project, false) },
        ]}
        onCancel={() => setDialog(null)}
        onDone={() => setDialog(null)}
      />
    </>
  )
})

interface Choice {
  label: string
  /** The default button (Return). */
  primary?: boolean
  destructive?: boolean
  onSelect: () => void
}

/// `.confirmationDialog` with more than one action (the worktree merge and
/// remove questions): the title, the message and the actions with Cancel,
/// as a sheet.
function ChoiceSheet({ open, title, message, choices, onCancel, onDone }: { open: boolean; title: string; message: string; choices: Choice[]; onCancel: () => void; onDone: () => void }) {
  const primaryRef = useRef<HTMLButtonElement>(null)
  return (
    <Sheet
      open={open}
      onClose={onCancel}
      title={title}
      width={460}
      initialFocus={primaryRef}
      footer={
        <>
          <Button variant="bordered" onClick={onCancel}>
            Cancel
          </Button>
          {choices.map((choice) => (
            <Button
              key={choice.label}
              ref={choice.primary ? primaryRef : undefined}
              variant={choice.destructive ? 'destructive' : choice.primary ? 'prominent' : 'bordered'}
              onClick={() => {
                onDone()
                choice.onSelect()
              }}
            >
              {choice.label}
            </Button>
          ))}
        </>
      }
    >
      <p className={styles.sheetMessage}>{message}</p>
    </Sheet>
  )
}

// MARK: - Loading rows

/// `OpeningProjectRow`: a folder whose `project/open` has not returned yet
/// (the remembered projects at launch, or one just opened): its name muted
/// with a spinner where the badge sits. Not clickable; the real row replaces it.
export const OpeningProjectRow = memo(function OpeningProjectRow({ path, name }: { path: string; name: string }) {
  return (
    <Tooltip content={`Opening ${path}`}>
      <div className={rowClass(false, false)} role="status" aria-label={`Opening project ${name}`}>
        <RowIcon icon="folder" tint="tertiary" />
        <span className={[styles.title, styles.muted].join(' ')}>{name}</span>
        <span className={styles.spacer} />
        <BrailleSpinner size={11} tint="var(--text-tertiary)" />
      </div>
    </Tooltip>
  )
})

/** The bar widths of `SessionRowsSkeleton`, as fractions of the row. */
const SKELETON_WIDTHS = [0.78, 0.6, 0.86, 0.5, 0.7, 0.64]

/// `SessionRowsSkeleton`: placeholder chat rows while the chat list loads:
/// bars of varied width in the 30 px row, pulsing together.
export function SessionRowsSkeleton({ count = 5 }: { count?: number }) {
  return (
    <Skeleton label="Loading chats" className={styles.skeleton}>
      {Array.from({ length: count }, (_, index) => (
        <div key={index} className={rowClass(false, false)}>
          <Skeleton.Bar width={`${(SKELETON_WIDTHS[index % SKELETON_WIDTHS.length] ?? 0.7) * 100}%`} height={9} />
        </div>
      ))}
    </Skeleton>
  )
}

// MARK: - PinnedRecentRow

/** `PinnedRecentRow`: a pinned recent project that is not open: muted; a click opens it. */
export const PinnedRecentRow = memo(function PinnedRecentRow({ recent, ready }: { recent: RecentProject; ready: boolean }) {
  const [hovering, hover] = useHover()
  const open = () => void useApp.getState().openProject(recent.path)
  const menu = (): MenuItem[] => [
    { label: 'Open', onSelect: open },
    { label: 'Unpin', onSelect: () => RecentProjects.setPinned(false, recent.path) },
    { label: revealLabel, onSelect: () => revealFolder(recent.path) },
    { type: 'separator' },
    { label: 'Remove from Recents', onSelect: () => RecentProjects.remove(recent.path) },
  ]
  return (
    <ContextMenu items={menu}>
      <Tooltip content={abbreviatedPath(recent.path)}>
        <button
          type="button"
          className={rowClass(false, hovering && ready, ready ? '' : styles.disabledRow)}
          disabled={!ready}
          aria-label={`Pinned project ${recent.name}, not open`}
          onClick={open}
          {...hover}
        >
          <RowIcon icon="folder" tint="tertiary" />
          <MiddleTruncate text={recent.name} className={styles.pinnedName} />
          <Icon name="pinned" size={10} strokeWidth={2} className={styles.pinGlyph} />
        </button>
      </Tooltip>
    </ContextMenu>
  )
})

// MARK: - SessionRow

/** What the chat row shows at its trailing edge. */
export type SessionIndicator =
  /** The run waits on the user (approval, questions) or ended blocked: an orange dot. */
  | { kind: 'help'; label: string }
  | { kind: 'running' }
  /** Finished and not opened yet (blue), failed or errored (red), blocked / approval (orange). */
  | { kind: 'unseen'; tint: 'accent' | 'danger' | 'warning'; label: string }
  | null

export interface SessionRowProps {
  session: SessionSummary
  isSelected: boolean
  isRunning: boolean
  indicator: SessionIndicator
}

/// `SessionRow`: one chat of the selected project: the truncated title, the
/// relative time on hover, a spinner while it runs, an orange dot while it
/// needs the user, a blue dot when it finished unseen and a red one after an
/// error; inline rename (Return commits, Esc cancels, double-click starts
/// it) and a destructive delete confirmation.
export const SessionRow = memo(function SessionRow({ session, isSelected, isRunning, indicator }: SessionRowProps) {
  const [hovering, hover] = useHover()
  /** Over the dot / spinner, whose own help says why: the row's title tooltip stands down. */
  const [overIndicator, setOverIndicator] = useState(false)
  const [renaming, setRenaming] = useState(false)
  const [draftTitle, setDraftTitle] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  const renamingRef = useRef(false)

  useEffect(() => {
    if (!renaming) return
    // The field is attached a beat after `renaming` flips; focus then sticks.
    const timer = window.setTimeout(() => {
      const input = inputRef.current
      if (!input) return
      input.focus()
      input.setSelectionRange(input.value.length, input.value.length)
    }, 40)
    return () => window.clearTimeout(timer)
  }, [renaming])

  const startRename = () => {
    setDraftTitle(session.title === 'New chat' ? '' : session.title)
    renamingRef.current = true
    setRenaming(true)
  }

  const commitRename = () => {
    if (!renamingRef.current) return
    renamingRef.current = false
    setRenaming(false)
    const title = draftTitle.trim()
    if (title === session.title) return
    void useApp.getState().renameSession(session.id, title)
  }

  const cancelRename = () => {
    renamingRef.current = false
    setRenaming(false)
    setDraftTitle('')
  }

  const askDelete = async () => {
    const confirmed = await confirm({
      title: `Delete “${session.title}”?`,
      message: 'Its tasks and history are removed.',
      confirmLabel: 'Delete',
      destructive: true,
    })
    if (confirmed) await useApp.getState().deleteSession(session.id)
  }

  const menu = (): MenuItem[] => [
    { label: 'Rename', onSelect: startRename },
    { label: 'Copy Title', onSelect: () => useApp.getState().copyToPasteboard(session.title) },
    { type: 'separator' },
    {
      label: 'Delete…',
      destructive: true,
      disabled: isRunning,
      help: isRunning ? 'Finish or cancel the running task first' : 'Delete this chat',
      onSelect: () => void askDelete(),
    },
  ]

  const parts = [session.title]
  if (isRunning) parts.push('running')
  if (indicator && indicator.kind !== 'running') parts.push(indicator.label)

  if (renaming) {
    // No button while editing, so clicks reach the text field.
    return (
      <div className={rowClass(isSelected, hovering, styles.sessionRow)} {...hover}>
        <input
          ref={inputRef}
          className={styles.renameField}
          value={draftTitle}
          placeholder="Chat title"
          aria-label="Rename chat"
          spellCheck={false}
          onChange={(event) => setDraftTitle(event.target.value)}
          onBlur={commitRename}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing) return
            if (event.key === 'Enter') {
              event.preventDefault()
              commitRename()
            } else if (event.key === 'Escape') {
              event.preventDefault()
              event.stopPropagation()
              cancelRename()
            }
          }}
        />
        <Kbd>↩</Kbd>
      </div>
    )
  }

  return (
    <ContextMenu items={menu}>
      <Tooltip content={isRunning ? `Running · ${session.title}` : session.title} disabled={overIndicator}>
        <button
          type="button"
          className={rowClass(isSelected, hovering, styles.sessionRow)}
          aria-label={parts.join(', ')}
          aria-current={isSelected ? 'true' : undefined}
          onClick={() => void useApp.getState().selectSession(session.id)}
          onDoubleClick={startRename}
          onPointerEnter={hover.onPointerEnter}
          onPointerLeave={() => {
            hover.onPointerLeave()
            setOverIndicator(false)
          }}
        >
          <span className={styles.title}>{session.title}</span>
          <span className={styles.spacer} />
          {indicator !== null ? (
            <span className={styles.indicatorSlot} onPointerEnter={() => setOverIndicator(true)} onPointerLeave={() => setOverIndicator(false)}>
              <SessionTrailing indicator={indicator} />
            </span>
          ) : null}
          {hovering && !isRunning ? <span className={styles.relativeTime}>{TranscriptFormat.relative(sessionUpdatedDate(session))}</span> : null}
        </button>
      </Tooltip>
    </ContextMenu>
  )
})

/** The dot or spinner of a chat row. A run waiting on the user is still running; the dot says why it stopped. */
function SessionTrailing({ indicator }: { indicator: NonNullable<SessionIndicator> }) {
  if (indicator.kind === 'running') {
    return (
      <Tooltip content="Running">
        <BrailleSpinner size={11} />
      </Tooltip>
    )
  }
  const tint = indicator.kind === 'help' ? 'warning' : indicator.tint
  return (
    <Tooltip content={indicator.label}>
      <span className={styles.dotBox} aria-hidden="true">
        <span className={[styles.dot, styles[tint]].join(' ')} />
      </span>
    </Tooltip>
  )
}
