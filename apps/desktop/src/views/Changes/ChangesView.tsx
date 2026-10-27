import { memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from 'react'
import { gitFileCode, gitStagedFiles, gitUnstagedFiles, type GitFileStatus, type GitLogEntry, type GitStatus } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { ContextMenu } from '../../components/ContextMenu'
import { CopyButton } from '../../components/CopyButton'
import { confirm } from '../../components/dialogs'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { IconButton } from '../../components/IconButton'
import { Menu, type MenuItem } from '../../components/Menu'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { prefersReducedMotion } from '../../components/overlays'
import { formatShortcut, revealLabel } from '../../components/platform'
import { Tooltip } from '../../components/Tooltip'
import { useApp, useProjectValue, type PathAction } from '../../state/app'
import { ChatState } from '../../state/chat'
import {
  checkout,
  commitFromBox,
  discard,
  fetch,
  generateCommitMessage,
  loadBranches,
  loadCommitLog,
  pull,
  selectChangeInSection,
  stage,
  undoLastCommit,
  unstage,
} from '../../state/git'
import type { ProjectState } from '../../state/project'
import { selectedProject } from '../../state/selectors'
import { Icon, type IconName } from '../../theme/icons'
import { DiffView } from './DiffView'
import { CommitFileSheet, NewBranchSheet, PushSheet } from './GitSheets'
import styles from './ChangesView.module.css'

// Port of apps/macos/ContextAgent/Views/Changes/ChangesView.swift: the
// Changes tab (VS Code-style source control). Left, the list pane (300 px,
// narrower down to 220 px in a small window): header, branch / sync row,
// commit box, Staged Changes and Changes sections, recent commits. Right,
// the selected file's diff (`DiffView`). Every action goes through
// state/git.ts (one backend request under `project.gitBusy`).

// MARK: - ChangesView

/**
 * `ChangesView` (Views/Changes/ChangesView.swift): the Changes tab of the
 * selected project, or "No project open". The store refreshes the status
 * on `file_changed` / `task/finished`.
 */
export function ChangesView() {
  const project = useApp(selectedProject)
  return (
    <div className={styles.changesView}>
      {project ? (
        <ChangesContent key={project.id} project={project} />
      ) : (
        <EmptyPlaceholder
          icon="changes"
          title="No project open"
          subtitle="Open a repository to review its working tree changes."
          actionTitle="Open a project…"
          onAction={() => void useApp.getState().chooseProject()}
        />
      )}
    </div>
  )
}

// MARK: - Content

/** `ChangesContent`: not a repository, the split, the status error or "Reading git status…". */
function ChangesContent({ project }: { project: ProjectState }) {
  const isGitRepository = useProjectValue(project, (p) => p.isGitRepository, false)
  const status = useProjectValue(project, (p) => p.gitStatus, null)
  const gitError = useProjectValue(project, (p) => p.gitError, null)

  // `.task(id: project.id) { await initialLoad() }`
  useEffect(() => {
    const app = useApp.getState()
    if (project.gitStatus === null && project.gitError === null) {
      void app.refreshGit(project)
    } else if (project.selectedChangePath !== null && project.diff === null && project.diffError === null && !project.diffLoading) {
      void app.selectChange(project.selectedChangePath, project)
    }
  }, [project])

  if (!isGitRepository) {
    return <EmptyPlaceholder icon="changes" title="Not a git repository" subtitle="Open a folder with a .git directory to review changes here." />
  }
  if (status) {
    // A clean tree still shows the branch, sync and commit controls.
    return <Split project={project} status={status} />
  }
  if (gitError !== null) {
    return (
      <EmptyPlaceholder
        icon="warning"
        title="Could not read git status"
        subtitle={gitError}
        actionTitle="Retry"
        onAction={() => void useApp.getState().refreshGit(project)}
      />
    )
  }
  return (
    <div className={styles.loading}>
      <BrailleSpinner size={16} />
      <span className={styles.loadingText}>Reading git status…</span>
    </div>
  )
}

/** The width of an element, kept current with a `ResizeObserver` (`onGeometryChange`). */
function useElementWidth(ref: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0)
  useEffect(() => {
    const element = ref.current
    if (!element) return
    setWidth(element.getBoundingClientRect().width)
    const observer = new ResizeObserver((entries) => {
      const entry = entries[entries.length - 1]
      if (entry) setWidth(entry.contentRect.width)
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [ref])
  return width
}

/** `listWidth` / `listMinimum`. */
const LIST_WIDTH = 300
const LIST_MINIMUM = 220

/// `split(_:)`: 300 px list when the pane has room; narrower (down to
/// 220 px) when it does not, so the list and diff never overflow the pane.
function Split({ project, status }: { project: ProjectState; status: GitStatus }) {
  const ref = useRef<HTMLDivElement>(null)
  const paneWidth = useElementWidth(ref)
  const listWidth = paneWidth > 0 ? Math.min(LIST_WIDTH, Math.max(LIST_MINIMUM, Math.round(paneWidth * 0.45))) : LIST_WIDTH
  const selectedPath = useProjectValue(project, (p) => p.selectedChangePath, null)
  const file = status.files.find((candidate) => candidate.path === selectedPath) ?? null
  return (
    <div ref={ref} className={styles.split}>
      <div className={styles.listPane} style={{ width: listWidth }}>
        <ChangeListPane project={project} status={status} />
      </div>
      <div className={styles.diffPane}>
        <ChangeDiffPane project={project} file={file} />
      </div>
    </div>
  )
}

// MARK: - File list

/// `ChangeSection`: which section a row sits in; a file with both staged
/// and working-tree changes (`MM`) appears in both, with the diff following
/// the section.
type ChangeSection = 'staged' | 'changes'

/** `DiscardTarget`: what a discard confirmation covers. */
type DiscardTarget = { kind: 'paths'; paths: string[] } | { kind: 'all'; count: number }

/** The last path component (`(path as NSString).lastPathComponent`). */
function lastPathComponent(path: string): string {
  const slash = path.lastIndexOf('/')
  return slash < 0 ? path : path.slice(slash + 1)
}

/** `(path as NSString).deletingLastPathComponent`, "" for a top-level file. */
function directoryOf(path: string): string {
  const slash = path.lastIndexOf('/')
  if (slash < 0) return ''
  const dir = path.slice(0, slash)
  return dir === '.' ? '' : dir
}

/** `discardTitle`. */
function discardTitle(target: DiscardTarget): string {
  if (target.kind === 'all') return `Discard all ${target.count} changes?`
  const [only] = target.paths
  if (target.paths.length === 1 && only !== undefined) return `Discard changes to ${lastPathComponent(only)}?`
  return `Discard changes to ${target.paths.length} files?`
}

/** The id a row scrolls to by (`.id("\(section)|\(file.path)")`). */
function rowId(section: ChangeSection, path: string): string {
  return `${section}|${path}`
}

/// `ChangeListPane`: header, branch / sync row, the commit box, then the
/// Staged Changes and Changes sections (hover + / − / discard per row,
/// Stage all / Unstage all / Discard all per section) and the recent
/// commits. Sheets: push, new branch, commit one file.
function ChangeListPane({ project, status }: { project: ProjectState; status: GitStatus }) {
  const gitError = useProjectValue(project, (p) => p.gitError, null)
  const gitBusy = useProjectValue(project, (p) => p.gitBusy, null)
  const commitLog = useProjectValue(project, (p) => p.commitLog, [] as GitLogEntry[])
  const selectedPath = useProjectValue(project, (p) => p.selectedChangePath, null)
  const showStaged = useProjectValue(project, (p) => p.showStaged, false)
  const askCommitAll = useProjectValue(project, (p) => p.askCommitAll, false)

  const listRef = useRef<HTMLDivElement>(null)
  const [pushSheet, setPushSheet] = useState(false)
  const [newBranchSheet, setNewBranchSheet] = useState(false)
  const [commitFile, setCommitFile] = useState<string | null>(null)

  const staged = useMemo(() => gitStagedFiles(status), [status])
  const unstaged = useMemo(() => gitUnstagedFiles(status), [status])
  /** Keyboard order: staged rows, then working-tree rows. */
  const rows = useMemo(
    () => [
      ...staged.map((file) => ({ file, section: 'staged' as ChangeSection })),
      ...unstaged.map((file) => ({ file, section: 'changes' as ChangeSection })),
    ],
    [staged, unstaged],
  )

  // `.task(id: project.id)`: the log and the branches.
  useEffect(() => {
    void (async () => {
      await loadCommitLog(project)
      await loadBranches(project)
    })()
  }, [project])

  /** The discard `confirmationDialog`. */
  const requestDiscard = useCallback(
    (target: DiscardTarget) => {
      void (async () => {
        const ok = await confirm({
          title: discardTitle(target),
          message: 'Modified files go back to their last staged or committed contents; untracked files are deleted. This cannot be undone.',
          confirmLabel: 'Discard Changes',
          destructive: true,
        })
        if (!ok) return
        await discard(target.kind === 'paths' ? target.paths : [], project)
      })()
    },
    [project],
  )

  /** The "Undo the last commit?" `confirmationDialog`. */
  const requestUndoCommit = useCallback(() => {
    void (async () => {
      const subject = ChatState.clip(project.gitStatus?.head_subject ?? '', 60)
      const ok = await confirm({
        title: 'Undo the last commit?',
        message: `“${subject}” is removed from the branch; its changes stay staged. Already pushed commits are not undone on the remote.`,
        confirmLabel: 'Undo Last Commit',
      })
      if (ok) await undoLastCommit(project)
    })()
  }, [project])

  // `.confirmationDialog("Nothing is staged.", isPresented: $project.askCommitAll)`.
  useEffect(() => {
    if (!askCommitAll || !project.askCommitAll) return
    project.update((p) => (p.askCommitAll = false))
    const count = project.gitStatus ? gitUnstagedFiles(project.gitStatus).length : 0
    void (async () => {
      const ok = await confirm({
        title: 'Nothing is staged.',
        message: `Stage all ${count} changed file${count === 1 ? '' : 's'} and commit them?`,
        confirmLabel: 'Stage All and Commit',
      })
      if (ok) await commitFromBox(project, { all: true })
    })()
  }, [askCommitAll, project])

  /** `select(_:section:)`: focuses the list and shows the row's diff. */
  const select = useCallback(
    (path: string, section: ChangeSection) => {
      listRef.current?.focus({ preventScroll: true })
      void selectChangeInSection(path, section === 'staged', project)
    },
    [project],
  )

  const rowAction = useCallback(
    (action: 'stage' | 'unstage' | 'discard', path: string) => {
      if (action === 'stage') void stage([path], project)
      else if (action === 'unstage') void unstage([path], project)
      else requestDiscard({ kind: 'paths', paths: [path] })
    },
    [project, requestDiscard],
  )

  const contextAction = useCallback(
    (path: string, action: PathAction) => {
      useApp.getState().reveal(path, project, action)
    },
    [project],
  )

  /** `onMoveCommand`: up / down move the selection through the rows. */
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    if (event.altKey || event.metaKey || event.ctrlKey || event.shiftKey) return
    event.preventDefault()
    if (rows.length === 0) return
    const current = rows.findIndex((row) => row.file.path === selectedPath && showStaged === (row.section === 'staged'))
    const next =
      event.key === 'ArrowDown' ? (current < 0 ? 0 : Math.min(current + 1, rows.length - 1)) : current < 0 ? 0 : Math.max(current - 1, 0)
    if (next === current) return
    const target = rows[next]
    if (!target) return
    const element = listRef.current?.querySelector<HTMLElement>(`[data-row-id="${CSS.escape(rowId(target.section, target.file.path))}"]`)
    element?.scrollIntoView({ block: 'nearest', behavior: prefersReducedMotion() ? 'auto' : 'smooth' })
    select(target.file.path, target.section)
  }

  const busy = gitBusy !== null

  return (
    <div className={styles.listColumn} role="group" aria-label="Changed files">
      <ListHeader project={project} count={status.files.length} busy={gitBusy} />
      <BranchRow project={project} status={status} busy={busy} onPush={() => setPushSheet(true)} onNewBranch={() => setNewBranchSheet(true)} />
      <CommitBox project={project} status={status} busy={busy} onUndo={requestUndoCommit} />
      {gitError !== null ? <ErrorBanner error={gitError} onRetry={() => void useApp.getState().refreshGit(project)} /> : null}
      <div ref={listRef} className={styles.list} tabIndex={0} onKeyDown={onKeyDown}>
        <div className={styles.listContent}>
          {status.files.length === 0 ? <CleanRow /> : null}
          {staged.length > 0 ? (
            <>
              <SectionHeader title="Staged Changes" count={staged.length}>
                <Tooltip content="Unstage all">
                  <IconButton icon="unstage" label="Unstage all changes" size={11} noTooltip onClick={() => void unstage([], project)} />
                </Tooltip>
              </SectionHeader>
              {staged.map((file) => (
                <ChangeRow
                  key={rowId('staged', file.path)}
                  file={file}
                  section="staged"
                  isSelected={file.path === selectedPath && showStaged}
                  hasActions={!busy}
                  onSelect={select}
                  onAction={rowAction}
                  onContextAction={contextAction}
                  onCommitFile={setCommitFile}
                />
              ))}
            </>
          ) : null}
          {unstaged.length > 0 ? (
            <>
              <SectionHeader title="Changes" count={unstaged.length}>
                <Tooltip content="Discard all changes…">
                  <IconButton
                    icon="discard"
                    label="Discard all changes"
                    size={11}
                    noTooltip
                    onClick={() => requestDiscard({ kind: 'all', count: unstaged.length })}
                  />
                </Tooltip>
                <Tooltip content="Stage all">
                  <IconButton icon="stage" label="Stage all changes" size={11} noTooltip onClick={() => void stage([], project)} />
                </Tooltip>
              </SectionHeader>
              {unstaged.map((file) => (
                <ChangeRow
                  key={rowId('changes', file.path)}
                  file={file}
                  section="changes"
                  isSelected={file.path === selectedPath && !showStaged}
                  hasActions={!busy}
                  onSelect={select}
                  onAction={rowAction}
                  onContextAction={contextAction}
                  onCommitFile={setCommitFile}
                />
              ))}
            </>
          ) : null}
          {commitLog.length > 0 ? (
            <>
              <SectionHeader title="Recent Commits" count={null}>
                <IconButton icon="push" label="Push…" size={11} onClick={() => setPushSheet(true)} />
              </SectionHeader>
              {commitLog.slice(0, 8).map((entry, index) => (
                <CommitRow key={entry.hash} entry={entry} isHead={index === 0} onUndo={requestUndoCommit} />
              ))}
            </>
          ) : null}
        </div>
      </div>
      <PushSheet project={project} status={status} open={pushSheet} onClose={() => setPushSheet(false)} />
      <NewBranchSheet project={project} open={newBranchSheet} onClose={() => setNewBranchSheet(false)} />
      <CommitFileSheet project={project} path={commitFile ?? ''} open={commitFile !== null} onClose={() => setCommitFile(null)} />
    </div>
  )
}

/** `header`: "Changes", the file count, the action in flight, Refresh. */
function ListHeader({ project, count, busy }: { project: ProjectState; count: number; busy: string | null }) {
  return (
    <div className={styles.header}>
      <span className={styles.headerTitle}>Changes</span>
      <Chip>{`${count}`}</Chip>
      {busy !== null ? (
        <Tooltip content={busy}>
          <span className={styles.busy} role="img" aria-label={busy}>
            <BrailleSpinner size={11} tint="var(--text-secondary)" />
          </span>
        </Tooltip>
      ) : null}
      <span className={styles.flexSpacer} />
      <IconButton
        icon="refresh"
        label="Refresh"
        size={14}
        onClick={() => {
          void (async () => {
            await useApp.getState().refreshGit(project)
            await loadCommitLog(project)
            await loadBranches(project)
          })()
        }}
      />
    </div>
  )
}

/// `branchRow`: the branch menu (switch / create), the ahead-behind chip,
/// fetch / pull / push; everything disabled while an action runs.
function BranchRow({
  project,
  status,
  busy,
  onPush,
  onNewBranch,
}: {
  project: ProjectState
  status: GitStatus
  busy: boolean
  onPush: () => void
  onNewBranch: () => void
}) {
  const branches = useProjectValue(project, (p) => p.branches, null)
  const menuAnchor = useRef<HTMLButtonElement>(null)
  const [menuOpen, setMenuOpen] = useState(false)

  const menuItems = (): MenuItem[] => {
    const items: MenuItem[] = []
    if (branches) {
      items.push({ type: 'header', label: 'Branches' })
      for (const branch of branches.local) {
        const current = branch.name === branches.current
        items.push({
          id: `local-${branch.name}`,
          label: branch.name,
          checked: current,
          disabled: current,
          onSelect: () => void checkout(branch.name, false, project),
        })
      }
      // A remote branch is always "<remote>/<branch>": the backend lets
      // `origin/HEAD` through as a bare "origin" (`%(refname:short)` of
      // refs/remotes/origin/HEAD), which is not a branch to switch to.
      const remoteOnly = branches.remote.filter(
        (remote) => remote.name.includes('/') && !branches.local.some((local) => local.name === remote.short),
      )
      if (remoteOnly.length > 0) {
        items.push({ type: 'header', label: 'Remote' })
        for (const remote of remoteOnly) {
          items.push({ id: `remote-${remote.name}`, label: remote.name, onSelect: () => void checkout(remote.short, false, project) })
        }
      }
      items.push({ type: 'separator' })
    }
    items.push({ label: 'New Branch…', onSelect: onNewBranch })
    items.push({ label: 'Refresh Branches', onSelect: () => void loadBranches(project) })
    return items
  }

  const branchHelp = `${status.branch ?? 'detached HEAD'} · ${status.upstream != null ? `tracks ${status.upstream}` : 'no upstream branch'}`
  const ahead = status.ahead ?? 0
  const behind = status.behind ?? 0

  return (
    <div className={styles.branchRow}>
      {/* `BranchLeadLayout`: the name truncates so the buttons always fit. */}
      <span className={styles.branchLead}>
        <Tooltip content={branchHelp}>
          <button
            ref={menuAnchor}
            type="button"
            className={styles.branchMenu}
            disabled={busy}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <Icon name="branch" size={12} strokeWidth={2} />
            <span className={styles.branchName}>{status.branch ?? 'detached HEAD'}</span>
            <Icon name="chevronDown" size={9} strokeWidth={2.5} className={styles.branchChevron} />
          </button>
        </Tooltip>
        {status.upstream != null ? (
          <Tooltip content={`${ahead} to push, ${behind} to pull (${status.upstream})`}>
            <Chip className={styles.aheadChip}>{`↑${ahead} ↓${behind}`}</Chip>
          </Tooltip>
        ) : null}
      </span>
      <Menu open={menuOpen} onClose={() => setMenuOpen(false)} items={menuOpen ? menuItems() : []} anchorRef={menuAnchor} aria-label="Branches" />
      <Tooltip content="Fetch all remotes">
        <IconButton icon="fetch" label="Fetch" size={13} noTooltip disabled={busy} onClick={() => void fetch(project)} />
      </Tooltip>
      <ContextMenu
        disabled={busy}
        items={[
          { label: 'Pull', onSelect: () => void pull(false, project) },
          { label: 'Pull (Rebase)', onSelect: () => void pull(true, project) },
        ]}
      >
        <Tooltip content="Pull (right-click: rebase)">
          <IconButton icon="pull" label="Pull" size={13} noTooltip disabled={busy} onClick={() => void pull(false, project)} />
        </Tooltip>
      </ContextMenu>
      <IconButton icon="push" label="Push…" size={13} disabled={busy} onClick={onPush} />
    </div>
  )
}

/// `CommitBox`: the message editor (placeholder names the branch), ✨ model
/// generation, and Commit with a menu for Commit All / Amend / Undo.
function CommitBox({ project, status, busy, onUndo }: { project: ProjectState; status: GitStatus; busy: boolean; onUndo: () => void }) {
  const message = useProjectValue(project, (p) => p.commitMessage, '')
  const generating = useProjectValue(project, (p) => p.generatingMessage, false)
  const menuAnchor = useRef<HTMLButtonElement>(null)
  const [menuOpen, setMenuOpen] = useState(false)

  const messageEmpty = message.trim().length === 0
  const noChanges = status.files.length === 0
  /** Enabled with a message and any change (nothing staged asks "stage all?"). */
  const canCommit = !messageEmpty && !noChanges
  const hasHead = status.head_subject != null
  const shortcut = formatShortcut('mod+Enter')

  const items: MenuItem[] = [
    { label: 'Commit', disabled: !canCommit, onSelect: () => void commitFromBox(project) },
    { label: 'Commit All (Stage Everything)', disabled: messageEmpty || noChanges, onSelect: () => void commitFromBox(project, { all: true }) },
    { label: 'Commit (Amend Last)', disabled: messageEmpty || !hasHead, onSelect: () => void commitFromBox(project, { amend: true }) },
    { type: 'separator' },
    { label: 'Undo Last Commit…', disabled: !hasHead, onSelect: onUndo },
  ]

  return (
    <div className={styles.commitBox}>
      <textarea
        className={styles.commitEditor}
        value={message}
        placeholder={`Message (${shortcut} to commit on ${status.branch ?? 'HEAD'})`}
        aria-label="Commit message"
        spellCheck={false}
        onChange={(event) => {
          const text = event.target.value
          project.update((p) => (p.commitMessage = text))
        }}
      />
      <div className={styles.commitActions}>
        <Tooltip content={noChanges ? 'No changes to describe' : 'Write the message from the staged changes with the model'}>
          <IconButton
            icon="generate"
            label="Generate commit message"
            size={13}
            noTooltip
            disabled={busy || generating || noChanges}
            onClick={() => void generateCommitMessage(project)}
          />
        </Tooltip>
        {generating ? <BrailleSpinner size={11} tint="var(--text-secondary)" /> : null}
        <span className={styles.flexSpacer} />
        <Tooltip content={canCommit ? `Commit the staged changes (${shortcut})` : 'Type a message and stage changes to commit'}>
          <Button variant="prominent" size="small" className={styles.commitButton} disabled={busy || !canCommit} onClick={() => void commitFromBox(project)}>
            Commit
          </Button>
        </Tooltip>
        <Tooltip content="More commit options">
          <button
            ref={menuAnchor}
            type="button"
            className={styles.commitChevron}
            disabled={busy}
            aria-label="Commit options"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <Icon name="chevronDown" size={10} strokeWidth={2.5} />
          </button>
        </Tooltip>
        <Menu open={menuOpen} onClose={() => setMenuOpen(false)} items={items} anchorRef={menuAnchor} aria-label="Commit options" />
      </div>
    </div>
  )
}

/** `errorBanner(_:)`: the status error above the list, with Retry. */
function ErrorBanner({ error, onRetry }: { error: string; onRetry: () => void }) {
  return (
    <div className={styles.errorBanner} role="alert">
      <Icon name="warning" size={11} strokeWidth={2.25} className={styles.errorIcon} />
      <span className={styles.errorText}>{error}</span>
      <button type="button" className={styles.retry} onClick={onRetry}>
        Retry
      </button>
    </div>
  )
}

/** `cleanRow`: "Working tree clean" inside the list, so the controls stay. */
function CleanRow() {
  return (
    <div className={styles.cleanRow}>
      <Icon name="check" size={14} className={styles.cleanIcon} />
      <span className={styles.cleanText}>Working tree clean</span>
    </div>
  )
}

/** `SectionHeader`: "STAGED CHANGES 3" with the section's actions at the right. */
function SectionHeader({ title, count, children }: { title: string; count: number | null; children?: ReactNode }) {
  return (
    <div className={styles.sectionHeader} role="group" aria-label={count !== null ? `${title}, ${count}` : title}>
      <span className={styles.sectionTitle}>{title.toUpperCase()}</span>
      {count !== null ? <span className={styles.sectionCount}>{count}</span> : null}
      <span className={styles.flexSpacer} />
      {children}
    </div>
  )
}

/** `CommitRow`: short hash and subject; the HEAD row can be undone. */
const CommitRow = memo(function CommitRow({ entry, isHead, onUndo }: { entry: GitLogEntry; isHead: boolean; onUndo: () => void }) {
  const items = (): MenuItem[] => {
    const list: MenuItem[] = [
      { label: 'Copy Hash', onSelect: () => useApp.getState().copyToPasteboard(entry.hash) },
      { label: 'Copy Subject', onSelect: () => useApp.getState().copyToPasteboard(entry.subject) },
    ]
    if (isHead) {
      list.push({ type: 'separator' })
      list.push({ label: 'Undo Last Commit…', onSelect: onUndo })
    }
    return list
  }
  return (
    <ContextMenu items={items}>
      <Tooltip content={`${entry.author} · ${entry.date}`}>
        <div className={styles.commitRow} role="group" aria-label={`Commit ${entry.short}, ${entry.subject}`}>
          <span className={styles.commitHash}>{entry.short}</span>
          <span className={styles.commitSubject}>{entry.subject}</span>
        </div>
      </Tooltip>
    </ContextMenu>
  )
})

interface ChangeRowProps {
  file: GitFileStatus
  section: ChangeSection
  isSelected: boolean
  /// Shows the hover actions (stage / unstage / discard) instead of the line
  /// counts while hovered; false while a git action runs.
  hasActions: boolean
  onSelect: (path: string, section: ChangeSection) => void
  onAction: (action: 'stage' | 'unstage' | 'discard', path: string) => void
  onContextAction: (path: string, action: PathAction) => void
  onCommitFile: (path: string) => void
}

/** A hover action on a changed-file row (`RowAction`). */
interface RowAction {
  id: 'stage' | 'unstage' | 'discard'
  icon: IconName
  label: string
}

const STAGED_ACTIONS: RowAction[] = [{ id: 'unstage', icon: 'unstage', label: 'Unstage' }]
const CHANGES_ACTIONS: RowAction[] = [
  { id: 'discard', icon: 'discard', label: 'Discard changes' },
  { id: 'stage', icon: 'stage', label: 'Stage' },
]

/// `ChangeRow`: status letter, file name over its folder, the line counts at
/// the right edge (replaced by the row actions on hover), the staged dot.
/// One background for selection and hover; the path is in the row itself,
/// so no tooltip either.
const ChangeRow = memo(function ChangeRow({ file, section, isSelected, hasActions, onSelect, onAction, onContextAction, onCommitFile }: ChangeRowProps) {
  const code = gitFileCode(file)
  const name = lastPathComponent(file.path)
  const directory = directoryOf(file.path)
  const actions = section === 'staged' ? STAGED_ACTIONS : CHANGES_ACTIONS
  const added = file.insertions ?? null
  const removed = file.deletions ?? null
  const changes = lineChangesLabel(file)

  const menuItems = (): MenuItem[] => {
    const items: MenuItem[] = []
    if (section === 'changes') {
      items.push({ label: 'Stage', onSelect: () => onAction('stage', file.path) })
      items.push({ label: 'Discard Changes…', onSelect: () => onAction('discard', file.path) })
    } else {
      items.push({ label: 'Unstage', onSelect: () => onAction('unstage', file.path) })
    }
    items.push({ label: 'Commit This File…', onSelect: () => onCommitFile(file.path) })
    items.push({ type: 'separator' })
    items.push({ label: 'Open in Editor', onSelect: () => onContextAction(file.path, 'openInEditor') })
    items.push({ label: revealLabel, onSelect: () => onContextAction(file.path, 'revealInFileManager') })
    items.push({ label: 'Open with Default Editor', onSelect: () => onContextAction(file.path, 'openWithEditor') })
    items.push({ label: 'Copy Path', onSelect: () => onContextAction(file.path, 'copyPath') })
    return items
  }

  return (
    <ContextMenu items={menuItems}>
      <div
        className={[styles.row, isSelected ? styles.rowSelected : '', hasActions ? styles.rowHasActions : ''].filter(Boolean).join(' ')}
        role="button"
        tabIndex={-1}
        data-row-id={rowId(section, file.path)}
        aria-current={isSelected || undefined}
        aria-label={`${changesStatusLabel(code)} ${file.path}${file.staged ? ', staged' : ''}${changes.length === 0 ? '' : `, ${changes}`}`}
        onClick={() => onSelect(file.path, section)}
      >
        <StatusLetter code={code} />
        <span className={styles.rowText}>
          <MiddleTruncate natural text={name} className={styles.rowName} />
          {directory.length > 0 ? <span className={styles.rowDirectory}>{`‎${directory}‎`}</span> : null}
        </span>
        <span className={styles.flexSpacer} />
        {hasActions ? (
          <span className={styles.rowActions}>
            {actions.map((action) => (
              <IconButton
                key={action.id}
                icon={action.icon}
                label={action.label}
                size={11}
                // Like an AppKit borderless button, a click does not take
                // focus: the list keeps it, so ↑ / ↓ go on working after a
                // row's button (and its row) went away.
                onMouseDown={(event) => event.preventDefault()}
                onClick={(event) => {
                  event.stopPropagation()
                  onAction(action.id, file.path)
                }}
              />
            ))}
          </span>
        ) : null}
        {added !== null || removed !== null ? (
          <span className={styles.rowCounts} aria-label={changes}>
            {added !== null && (added > 0 || removed === 0) ? <span className={styles.added}>{`+${added}`}</span> : null}
            {removed !== null && removed > 0 ? <span className={styles.removed}>{`−${removed}`}</span> : null}
          </span>
        ) : null}
        {file.staged ? (
          <Tooltip content="Staged">
            <span className={styles.stagedDot} role="img" aria-label="staged" />
          </Tooltip>
        ) : null}
      </div>
    </ContextMenu>
  )
})

/** `lineChangesLabel`: "12 lines added, 3 removed" for screen readers; empty when unknown. */
function lineChangesLabel(file: GitFileStatus): string {
  const parts: string[] = []
  const added = file.insertions ?? null
  const removed = file.deletions ?? null
  if (added !== null) parts.push(`${added} line${added === 1 ? '' : 's'} added`)
  if (removed !== null && removed > 0) parts.push(`${removed} removed`)
  return parts.join(', ')
}

/** `StatusLetter`: the git status letter in a small tinted square. */
function StatusLetter({ code }: { code: string }) {
  return (
    <span className={styles.statusLetter} style={{ ['--tint' as string]: changesStatusTint(code) }} aria-hidden>
      {code}
    </span>
  )
}

/** `ChangesStatus.label(for:)`: status-letter wording shared by the list, the diff header and the Inspector Files tab. */
export function changesStatusLabel(code: string): string {
  switch (code) {
    case 'M':
      return 'Modified'
    case 'A':
      return 'Added'
    case 'D':
      return 'Deleted'
    case 'R':
      return 'Renamed'
    case 'C':
      return 'Copied'
    case 'T':
      return 'Type changed'
    case 'U':
      return 'Unmerged'
    case '?':
      return 'Untracked'
    case '!':
      return 'Ignored'
    default:
      return 'Changed'
  }
}

/** `ChangesStatus.tint(for:)`: the status letter's colour as a CSS value. */
export function changesStatusTint(code: string): string {
  switch (code) {
    case 'M':
    case 'T':
      return 'var(--warning)'
    case 'A':
      return 'var(--success)'
    case 'D':
    case 'U':
      return 'var(--danger)'
    case 'R':
    case 'C':
      return 'var(--accent)'
    default:
      return 'var(--text-tertiary)'
  }
}

/** `ChangesStatus` (ChangesView.swift) under its Swift name. */
export const ChangesStatus = { label: changesStatusLabel, tint: changesStatusTint }

// MARK: - Diff pane

/** `DiffCounts.count(_:)`: +N / −N of a single-file diff for the pane header. */
function diffCounts(diff: string): { added: number; removed: number } {
  let added = 0
  let removed = 0
  for (const line of diff.split('\n')) {
    if (line.length === 0) continue
    const first = line.charCodeAt(0)
    if (first === 0x2b) {
      if (line.startsWith('+++')) continue
      added += 1
    } else if (first === 0x2d) {
      if (line.startsWith('---')) continue
      removed += 1
    }
  }
  return { added, removed }
}

/// `ChangeDiffPane`: the file header (path, status / counts / mode chips by
/// pane width, Copy path, Reveal) over the diff, or "Select a file".
function ChangeDiffPane({ project, file }: { project: ProjectState; file: GitFileStatus | null }) {
  const ref = useRef<HTMLDivElement>(null)
  const width = useElementWidth(ref)
  const diff = useProjectValue(project, (p) => p.diff, null)
  const diffError = useProjectValue(project, (p) => p.diffError, null)
  const diffLoading = useProjectValue(project, (p) => p.diffLoading, false)
  const showStaged = useProjectValue(project, (p) => p.showStaged, false)
  const diffText = diff?.diff ?? ''
  const counts = useMemo(() => (diffText.length === 0 ? null : diffCounts(diffText)), [diffText])

  return (
    <div ref={ref} className={styles.diffColumn}>
      {file ? (
        <>
          <DiffHeader project={project} file={file} width={width} counts={counts} loading={diffLoading} showStaged={showStaged} />
          {diff ? (
            diff.diff.length === 0 ? (
              <EmptyDiff file={file} showStaged={showStaged} />
            ) : (
              // The pane header names the file: no second file header inside the diff.
              <DiffView diff={diff.diff} truncated={diff.truncated} showFileHeaders={false} />
            )
          ) : diffError !== null ? (
            <EmptyPlaceholder
              icon="warning"
              title="Could not load the diff"
              subtitle={diffError}
              actionTitle="Retry"
              onAction={() => void useApp.getState().selectChange(file.path, project)}
            />
          ) : (
            <div className={styles.loading}>
              <BrailleSpinner size={16} />
              <span className={styles.loadingText}>Loading diff…</span>
            </div>
          )}
        </>
      ) : (
        <EmptyPlaceholder icon="file" title="Select a file" subtitle="Pick a changed file to see its diff." />
      )}
    </div>
  )
}

/** `header(_:)` of the diff pane. */
function DiffHeader({
  project,
  file,
  width,
  counts,
  loading,
  showStaged,
}: {
  project: ProjectState
  file: GitFileStatus
  width: number
  counts: { added: number; removed: number } | null
  loading: boolean
  showStaged: boolean
}) {
  const code = gitFileCode(file)
  const reveal = (action: PathAction) => useApp.getState().reveal(file.path, project, action)
  const items: MenuItem[] = [
    { label: 'Open in Editor', onSelect: () => reveal('openInEditor') },
    { label: revealLabel, onSelect: () => reveal('revealInFileManager') },
    { label: 'Open with Default Editor', onSelect: () => reveal('openWithEditor') },
    { label: 'Copy Path', onSelect: () => reveal('copyPath') },
  ]
  const showCounts = counts !== null && (counts.added > 0 || counts.removed > 0)

  return (
    <ContextMenu items={items}>
      <div className={styles.diffHeader}>
        {width >= 220 ? <Icon name="file" size={13} strokeWidth={2} className={styles.diffHeaderIcon} /> : null}
        {/* Takes what the fixed-size chips and buttons leave; the chips drop
            out by pane width so there is always room for some path. */}
        <MiddleTruncate natural text={file.path} className={`${styles.diffPath} selectable`} />
        {/* By pane width: the diff mode chip goes first, then the status chip, then the line counts. */}
        {width >= 300 ? (
          <span className={styles.headerChips}>
            {width >= 380 ? (
              <Chip variant="tinted" tint={changesStatusTint(code)}>
                {changesStatusLabel(code)}
              </Chip>
            ) : null}
            {showCounts && counts ? (
              <span className={styles.diffCounts} aria-label={`${counts.added} additions, ${counts.removed} deletions`}>
                <span className={styles.added}>{`+${counts.added}`}</span>
                <span className={styles.removed}>{`−${counts.removed}`}</span>
              </span>
            ) : null}
            {width >= 480 ? (
              <Tooltip content={showStaged ? 'git diff --cached' : 'git diff'}>
                <Chip>{showStaged ? 'staged diff' : 'working tree'}</Chip>
              </Tooltip>
            ) : null}
          </span>
        ) : null}
        {loading ? <BrailleSpinner /> : null}
        <span className={styles.diffHeaderSpacer} />
        <CopyButton text={file.path} label="Copy path" />
        <IconButton icon="folder" label={revealLabel} size={14} onClick={() => reveal('revealInFileManager')} />
      </div>
    </ContextMenu>
  )
}

/** `emptyDiff(_:)`: why an empty diff is empty. */
function EmptyDiff({ file, showStaged }: { file: GitFileStatus; showStaged: boolean }) {
  if (gitFileCode(file) === '?') {
    return (
      <EmptyPlaceholder
        icon="file"
        title="Untracked file"
        subtitle="git diff has nothing to show for untracked files. Stage it (git add) and switch on Staged to review its contents."
      />
    )
  }
  if (showStaged && !file.staged) {
    return <EmptyPlaceholder icon="changes" title="Nothing staged" subtitle="This file has no staged changes. Switch off Staged to see the working tree diff." />
  }
  if (!showStaged && file.staged && file.status.length === 1) {
    return <EmptyPlaceholder icon="changes" title="All changes are staged" subtitle="Switch on Staged to review them." />
  }
  return <EmptyPlaceholder icon="changes" title="No differences" subtitle="The diff is empty." />
}
