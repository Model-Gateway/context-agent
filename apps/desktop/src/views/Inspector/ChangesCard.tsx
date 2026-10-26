import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { Sheet } from '../../components/Sheet'
import { Tooltip } from '../../components/Tooltip'
import { useApp } from '../../state/app'
import { pull } from '../../state/git'
import type { ProjectState } from '../../state/project'
import { parentProject } from '../../state/selectors'
import { mergeWorktree } from '../../state/worktrees'
import { Icon } from '../../theme/icons'
import { CommitChangesSheet, PushSheet } from '../Changes/GitSheets'
import { InspectorCard } from './InspectorCard'
import { MiddleText } from './MiddleText'
import { cx } from './inspectorShared'
import styles from './ChangesCard.module.css'

export interface ChangesCardProps {
  project: ProjectState
}

/**
 * Port of `ChangesCard` (Views/Inspector/InspectorView.swift): the selected
 * project's working tree — changed files and lines, the branch against its
 * upstream, and the git step that applies next: commit when dirty, push when
 * ahead (publish without an upstream), pull when behind, merge into the
 * parent when the project is a worktree; Review opens the Changes tab.
 */
export function ChangesCard({ project }: ChangesCardProps) {
  const [commitSheet, setCommitSheet] = useState(false)
  const [pushSheet, setPushSheet] = useState(false)
  const [confirmMerge, setConfirmMerge] = useState(false)
  const parent = useApp((state) => parentProject(state, project))

  const status = project.gitStatus
  const fileCount = project.dirtyCount
  const lines = project.lineChanges
  const ahead = status?.ahead ?? 0
  const behind = status?.behind ?? 0
  const hasRemote = (status?.remotes ?? []).length > 0
  // Ahead of the upstream, or a branch with a remote but no upstream yet.
  const canPush = status != null && status.branch != null && hasRemote && (status.upstream == null || ahead > 0)
  const parentBranch = parent?.branch ?? parent?.name ?? 'parent'
  // A merge touches both checkouts and needs a clean worktree.
  const canMerge = fileCount === 0 && !project.isRunning && !(parent?.isRunning ?? false)
  const busy = project.gitBusy !== null
  const upstream = status?.upstream ?? null

  const actionsRef = useRef<HTMLDivElement>(null)
  const fitsInRow = useFitsInRow(actionsRef, 6)

  const pushLabel = upstream === null ? 'Publish…' : `Push ${ahead}…`
  const pushHelp = upstream === null ? 'Push the branch and set its upstream' : `Push ${ahead} commit${ahead === 1 ? '' : 's'} to ${upstream ?? 'the upstream'}`

  const actionButtons: ReactNode[] = []
  if (fileCount > 0) {
    actionButtons.push(
      <Tooltip key="commit" content="Commit the staged changes, or all of them when nothing is staged">
        <Button variant="prominent" size="small" icon="commit" className={styles.actionButton} disabled={busy} data-fit-item="" onClick={() => setCommitSheet(true)}>
          Commit…
        </Button>
      </Tooltip>,
    )
  }
  if (canPush) {
    // The primary action once everything is committed.
    actionButtons.push(
      <Tooltip key="push" content={pushHelp}>
        <Button variant={fileCount > 0 ? 'bordered' : 'prominent'} size="small" icon="push" className={styles.actionButton} disabled={busy} data-fit-item="" onClick={() => setPushSheet(true)}>
          {pushLabel}
        </Button>
      </Tooltip>,
    )
  }
  if (behind > 0) {
    actionButtons.push(
      <Tooltip key="pull" content={`Pull ${behind} commit${behind === 1 ? '' : 's'} from ${upstream ?? 'the upstream'}`}>
        <Button variant="bordered" size="small" icon="pull" className={styles.actionButton} disabled={busy} data-fit-item="" onClick={() => void pull(false, project)}>
          {`Pull ${behind}`}
        </Button>
      </Tooltip>,
    )
  }
  if (project.isWorktree) {
    actionButtons.push(
      <Tooltip
        key="merge"
        content={fileCount > 0 ? `Commit the worktree's changes before merging it into ${parentBranch}` : `Merge this worktree's branch into ${parentBranch}`}
      >
        <Button variant="bordered" size="small" icon="branch" className={styles.actionButton} disabled={busy || !canMerge} data-fit-item="" onClick={() => setConfirmMerge(true)}>
          Merge…
        </Button>
      </Tooltip>,
    )
  }
  if (fileCount > 0) {
    actionButtons.push(
      <Tooltip key="review" content="Open the Changes tab">
        <Button variant="plain" size="small" className={styles.reviewButton} disabled={busy} data-fit-item="" onClick={() => useApp.getState().setSidebarItem('changes')}>
          Review
        </Button>
      </Tooltip>,
    )
  }

  return (
    <InspectorCard title="Changes">
      {/* "3 files changed   +120 −4", or a clean-tree line. */}
      <div className={styles.summary}>
        {fileCount === 0 ? (
          <>
            <Icon name="check" size={12.5} strokeWidth={1.75} className={styles.cleanIcon} />
            <span className={styles.clean}>Working tree clean</span>
          </>
        ) : (
          <span className={styles.dirty}>{`${fileCount} file${fileCount === 1 ? '' : 's'} changed`}</span>
        )}
        <span className={styles.spacer} />
        {busy ? (
          <Tooltip content={project.gitBusy ?? ''}>
            <span className={styles.busy} aria-label={project.gitBusy ?? ''}>
              <BrailleSpinner size={11} tint="var(--text-secondary)" />
            </span>
          </Tooltip>
        ) : null}
        {fileCount > 0 ? (
          <span className={styles.lines} aria-label={`${lines.added} lines added, ${lines.removed} removed`}>
            <span className={styles.added} aria-hidden="true">{`+${lines.added}`}</span>
            <span className={styles.removed} aria-hidden="true">{`−${lines.removed}`}</span>
          </span>
        ) : null}
      </div>

      <div className={styles.branchRow}>
        <Icon name="branch" size={12} strokeWidth={2} className={styles.branchIcon} />
        <MiddleText text={project.branch ?? 'detached HEAD'} grow className={styles.branchName} />
        {upstream !== null ? (
          <Tooltip content={`${ahead} to push, ${behind} to pull (${upstream})`}>
            <Chip>{`↑${ahead} ↓${behind}`}</Chip>
          </Tooltip>
        ) : status?.branch != null && hasRemote ? (
          <Tooltip content="The branch has no upstream branch yet">
            <Chip>not published</Chip>
          </Tooltip>
        ) : null}
      </div>

      {actionButtons.length > 0 ? (
        <div ref={actionsRef} className={cx(styles.actions, fitsInRow ? styles.actionsRow : styles.actionsColumn)}>
          {actionButtons}
        </div>
      ) : null}

      {project.gitError !== null ? <p className={styles.error}>{project.gitError}</p> : null}

      <CommitChangesSheet project={project} open={commitSheet} onClose={() => setCommitSheet(false)} />
      {status ? <PushSheet project={project} status={status} open={pushSheet} onClose={() => setPushSheet(false)} /> : null}
      <MergeDialog project={project} parentBranch={parentBranch} open={confirmMerge} onClose={() => setConfirmMerge(false)} />
    </InspectorCard>
  )
}

/// `ViewThatFits(in: .horizontal) { HStack; VStack }`: whether the buttons
/// (`data-fit-item`) fit side by side, `gap` apart, in the container's width.
function useFitsInRow(ref: RefObject<HTMLDivElement | null>, gap: number): boolean {
  const [fits, setFits] = useState(true)
  const measure = useRef<() => void>(() => undefined)
  measure.current = () => {
    const element = ref.current
    if (!element) return
    const items = Array.from(element.querySelectorAll<HTMLElement>('[data-fit-item]'))
    const natural = items.reduce((sum, item) => sum + item.getBoundingClientRect().width, 0) + gap * Math.max(0, items.length - 1)
    setFits(natural <= element.clientWidth + 0.5)
  }
  useLayoutEffect(() => measure.current())
  useEffect(() => {
    const element = ref.current
    if (!element || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => measure.current())
    observer.observe(element)
    return () => observer.disconnect()
  }, [ref])
  return fits
}

// MARK: - Merge confirmation

/**
 * The `confirmationDialog` "Merge “<worktree>” into <parent>?": Merge,
 * Merge and Remove Worktree, Cancel.
 */
function MergeDialog({ project, parentBranch, open, onClose }: { project: ProjectState; parentBranch: string; open: boolean; onClose: () => void }) {
  const mergeRef = useRef<HTMLButtonElement>(null)
  const merge = (deleteAfter: boolean) => {
    onClose()
    void mergeWorktree(project, deleteAfter)
  }
  return (
    <Sheet
      open={open}
      onClose={onClose}
      width={420}
      title={`Merge “${project.worktreeLabel}” into ${parentBranch}?`}
      initialFocus={mergeRef}
      footer={
        <>
          <Button variant="bordered" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="bordered" onClick={() => merge(true)}>
            Merge and Remove Worktree
          </Button>
          <Button ref={mergeRef} variant="prominent" onClick={() => merge(false)}>
            Merge
          </Button>
        </>
      }
    >
      <p className={styles.mergeMessage}>
        {`The branch is merged into ${parentBranch} with a merge commit; a conflict leaves ${parentBranch} untouched. “Merge and Remove Worktree” also deletes the checkout and the branch.`}
      </p>
    </Sheet>
  )
}
