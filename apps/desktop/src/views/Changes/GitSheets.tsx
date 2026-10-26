import { useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { errorText } from '../../backend/errors'
import { gitFileCode, gitStagedFiles, type GitRemote, type GitStatus } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { Sheet } from '../../components/Sheet'
import { toasts } from '../../components/toasts'
import { Tooltip } from '../../components/Tooltip'
import { useProjectValue } from '../../state/app'
import { checkout, commit, generateCommitMessage, push } from '../../state/git'
import type { ProjectState } from '../../state/project'
import { Icon } from '../../theme/icons'
import styles from './GitSheets.module.css'

// Port of apps/macos/ContextAgent/Views/Changes/GitSheets.swift: the Changes
// tab's sheets — push (remote, branch, upstream, force-with-lease, git's
// output inline), a new branch, committing one file — and the Inspector's
// "Commit…" sheet. Each takes `open` (true by default, so a caller may also
// mount it only while shown) and `onClose` (`dismiss()`); the content mounts
// fresh on every open, like a SwiftUI sheet's `@State`.

/** The last path component (`(path as NSString).lastPathComponent`). */
function lastPathComponent(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  const slash = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  return slash < 0 ? trimmed : trimmed.slice(slash + 1)
}

/// Return in a single-line field (or on a checkbox / picker) runs the
/// sheet's default action (`.keyboardShortcut(.defaultAction)`); buttons
/// and multi-line editors keep their own Return. Attached as
/// `onKeyDownCapture`: the Sheet's focus trap stops Return from bubbling
/// past the card (so the composer and global shortcuts never see it), which
/// also keeps it from React's bubble-phase listener on the portal.
function defaultActionKeyDown(action: () => void, enabled: boolean) {
  return (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Enter' || event.shiftKey || event.altKey || event.metaKey || event.ctrlKey) return
    if (event.nativeEvent.isComposing) return
    const target = event.target as HTMLElement
    if (target.tagName === 'BUTTON' || target.tagName === 'TEXTAREA') return
    event.preventDefault()
    if (enabled) action()
  }
}

/// A multi-line editor that grows with its text between `minHeight` and
/// `maxHeight` (`TextEditor` with `.frame(minHeight:maxHeight:)`).
function GrowingTextEditor({
  value,
  onChange,
  minHeight,
  maxHeight,
  label,
}: {
  value: string
  onChange: (value: string) => void
  minHeight: number
  maxHeight: number
  label: string
}) {
  const ref = useRef<HTMLTextAreaElement>(null)
  useLayoutEffect(() => {
    const element = ref.current
    if (!element) return
    element.style.height = 'auto'
    element.style.height = `${Math.min(maxHeight, Math.max(minHeight, element.scrollHeight))}px`
  }, [value, minHeight, maxHeight])
  return (
    <textarea
      ref={ref}
      className={styles.textEditor}
      style={{ minHeight, maxHeight }}
      value={value}
      aria-label={label}
      spellCheck={false}
      onChange={(event) => onChange(event.target.value)}
    />
  )
}

// MARK: - PushSheet

export interface PushSheetProps {
  project: ProjectState
  status: GitStatus
  open?: boolean
  onClose: () => void
}

/**
 * `PushSheet` (GitSheets.swift): remote picker, branch, "Set as upstream"
 * (on when the branch has none), "Force with lease", git's output or error
 * inline; Push (Return) runs `git/push` and closes on success.
 */
export function PushSheet({ project, status, open = true, onClose }: PushSheetProps) {
  return (
    <Sheet open={open} onClose={onClose} width={480} aria-label="Push">
      {open ? <PushContent project={project} status={status} onClose={onClose} /> : null}
    </Sheet>
  )
}

function PushContent({ project, status, onClose }: { project: ProjectState; status: GitStatus; onClose: () => void }) {
  const remotes: GitRemote[] = status.remotes ?? []
  const [remote, setRemote] = useState(() => {
    const upstreamRemote = status.upstream != null ? status.upstream.split('/')[0] : undefined
    return upstreamRemote !== undefined && upstreamRemote.length > 0 ? upstreamRemote : (remotes[0]?.name ?? 'origin')
  })
  const [branch, setBranch] = useState(status.branch ?? '')
  const [setUpstream, setSetUpstream] = useState(status.upstream == null)
  const [force, setForce] = useState(false)
  const [output, setOutput] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [pushing, setPushing] = useState(false)

  const canPush = !pushing && remote.trim().length > 0 && branch.trim().length > 0

  const summary = (() => {
    const parts: string[] = []
    const ahead = status.ahead ?? 0
    const behind = status.behind ?? 0
    if (ahead > 0) parts.push(`${ahead} commit${ahead === 1 ? '' : 's'} to push`)
    if (behind > 0) parts.push(`${behind} behind ${status.upstream ?? 'upstream'}`)
    if (parts.length === 0) {
      return status.upstream != null ? `Push ${status.branch ?? 'HEAD'} to ${status.upstream}.` : `Publish ${status.branch ?? 'this branch'} to a remote.`
    }
    return `${parts.join(' · ')}.`
  })()

  const run = () => {
    setPushing(true)
    setError(null)
    setOutput(null)
    const target = remote.trim()
    const name = branch.trim()
    void (async () => {
      try {
        const text = await push(project, { remote: target, branch: name, setUpstream, force })
        toasts.push({ message: `Pushed ${name} to ${target}.`, level: 'success', details: text.length === 0 ? undefined : text })
        onClose()
      } catch (failure) {
        setError(errorText(failure))
      } finally {
        setPushing(false)
      }
    })()
  }

  return (
    <div className={styles.content} onKeyDownCapture={defaultActionKeyDown(run, canPush)}>
      <div className={styles.header}>
        <span className={styles.headerIcon} aria-hidden>
          <Icon name="push" size={22} strokeWidth={2} />
        </span>
        <div className={styles.headerText}>
          <div className={styles.title}>Push</div>
          <div className={styles.secondary}>{summary}</div>
        </div>
      </div>

      <div className={styles.grid}>
        <span className={styles.gridLabel}>Remote</span>
        {remotes.length === 0 ? (
          <input className={styles.textField} value={remote} placeholder="origin" aria-label="Remote" onChange={(event) => setRemote(event.target.value)} />
        ) : (
          <span className={styles.picker}>
            <select className={styles.pickerSelect} value={remote} aria-label="Remote" onChange={(event) => setRemote(event.target.value)}>
              {remotes.map((entry) => (
                <option key={entry.name} value={entry.name}>{`${entry.name}\u00a0\u00a0·\u00a0\u00a0${entry.url}`}</option>
              ))}
            </select>
            <Icon name="chevronDown" size={11} strokeWidth={2.5} className={styles.pickerChevron} />
          </span>
        )}
        <span className={styles.gridLabel}>Branch</span>
        <input className={styles.textField} value={branch} placeholder="branch" aria-label="Branch" onChange={(event) => setBranch(event.target.value)} />
        <span aria-hidden />
        <div className={styles.toggles}>
          <label className={styles.toggle}>
            <input type="checkbox" checked={setUpstream} onChange={(event) => setSetUpstream(event.target.checked)} />
            <span>Set as upstream (git push -u)</span>
          </label>
          <label className={styles.toggle}>
            <input type="checkbox" checked={force} onChange={(event) => setForce(event.target.checked)} />
            <span>Force with lease</span>
          </label>
          {force ? <div className={styles.warningCaption}>Overwrites the remote branch unless someone else pushed since you last fetched.</div> : null}
        </div>
      </div>

      {remotes.length === 0 ? (
        <div className={styles.warningCaption}>
          This repository has no remote yet; add one with <code className={styles.inlineCode}>git remote add origin &lt;url&gt;</code> first, or type its
          name above.
        </div>
      ) : null}

      {error !== null ? <OutputBox text={error} isError /> : output !== null ? <OutputBox text={output} isError={false} /> : null}

      <div className={styles.buttons}>
        <Button variant="bordered" onClick={onClose}>
          Cancel
        </Button>
        {pushing ? <BrailleSpinner size={13} className={styles.pushSpinner} /> : null}
        <span className={styles.spacer} />
        <Button variant="prominent" disabled={!canPush} onClick={run}>
          {force ? 'Force Push' : 'Push'}
        </Button>
      </div>
    </div>
  )
}

/** `outputBox(_:isError:)`: git's output or error, monospace, scrolling past 140 px. */
function OutputBox({ text, isError }: { text: string; isError: boolean }) {
  return (
    <div className={styles.outputBox}>
      <pre className={`${styles.output} ${isError ? styles.outputError : ''} selectable`}>{text}</pre>
    </div>
  )
}

// MARK: - NewBranchSheet

export interface NewBranchSheetProps {
  project: ProjectState
  open?: boolean
  onClose: () => void
}

/** `NewBranchSheet` (GitSheets.swift): a name, created from HEAD and switched to. */
export function NewBranchSheet({ project, open = true, onClose }: NewBranchSheetProps) {
  return (
    <Sheet open={open} onClose={onClose} width={400} aria-label="New Branch">
      {open ? <NewBranchContent project={project} onClose={onClose} /> : null}
    </Sheet>
  )
}

function NewBranchContent({ project, onClose }: { project: ProjectState; onClose: () => void }) {
  const [name, setName] = useState('')
  const valid = name.trim().length > 0

  const create = () => {
    const branch = name.trim()
    if (branch.length === 0) return
    onClose()
    void checkout(branch, true, project)
  }

  return (
    <div className={styles.content} onKeyDownCapture={defaultActionKeyDown(create, valid)}>
      <div className={styles.title}>New Branch</div>
      <div className={styles.secondary}>Created from the current commit and checked out. Uncommitted changes come along.</div>
      <input className={styles.textField} value={name} placeholder="feature/name" aria-label="Branch name" onChange={(event) => setName(event.target.value)} />
      <div className={styles.buttons}>
        <Button variant="bordered" onClick={onClose}>
          Cancel
        </Button>
        <span className={styles.spacer} />
        <Button variant="prominent" disabled={!valid} onClick={create}>
          Create and Switch
        </Button>
      </div>
    </div>
  )
}

// MARK: - CommitFileSheet

export interface CommitFileSheetProps {
  project: ProjectState
  /** The file, relative to the project root. */
  path: string
  open?: boolean
  onClose: () => void
}

/**
 * `CommitFileSheet` (GitSheets.swift): commits one file (staged first,
 * whatever else is staged) with its own message; the default message names
 * the file.
 */
export function CommitFileSheet({ project, path, open = true, onClose }: CommitFileSheetProps) {
  return (
    <Sheet open={open} onClose={onClose} width={440} aria-label={`Commit ${lastPathComponent(path)}`}>
      {open ? <CommitFileContent project={project} path={path} onClose={onClose} /> : null}
    </Sheet>
  )
}

function CommitFileContent({ project, path, onClose }: { project: ProjectState; path: string; onClose: () => void }) {
  const name = lastPathComponent(path)
  const [message, setMessage] = useState(() => {
    const file = project.gitStatus?.files.find((candidate) => candidate.path === path)
    const code = file ? gitFileCode(file) : null
    const verb = code === 'A' || code === '?' ? 'Add' : code === 'D' ? 'Remove' : 'Update'
    return `${verb} ${name}`
  })
  const [committing, setCommitting] = useState(false)
  const canCommit = !committing && message.trim().length > 0

  const run = () => {
    setCommitting(true)
    const text = message.trim()
    void (async () => {
      try {
        if ((await commit(text, project, { paths: [path] })) !== null) onClose()
      } finally {
        setCommitting(false)
      }
    })()
  }

  return (
    <div className={styles.content} onKeyDownCapture={defaultActionKeyDown(run, canCommit)}>
      <div className={styles.title}>{`Commit ${name}`}</div>
      <MiddleTruncate text={path} className={`${styles.path} selectable`} />
      <div className={styles.tertiaryCaption}>Only this file is committed; other staged or changed files stay as they are.</div>
      <GrowingTextEditor value={message} onChange={setMessage} minHeight={72} maxHeight={152} label="Commit message" />
      <div className={styles.buttons}>
        <Button variant="bordered" onClick={onClose}>
          Cancel
        </Button>
        {committing ? <BrailleSpinner size={13} /> : null}
        <span className={styles.spacer} />
        <Button variant="prominent" disabled={!canCommit} onClick={run}>
          Commit
        </Button>
      </div>
    </div>
  )
}

// MARK: - CommitChangesSheet

export interface CommitChangesSheetProps {
  project: ProjectState
  open?: boolean
  onClose: () => void
}

/**
 * `CommitChangesSheet` (GitSheets.swift): the Inspector's Changes card
 * "Commit…" — commits what is staged, or every change when nothing is.
 * Edits the Changes tab's draft (`project.commitMessage`), so a message
 * written in either place shows up in the other.
 */
export function CommitChangesSheet({ project, open = true, onClose }: CommitChangesSheetProps) {
  return (
    <Sheet open={open} onClose={onClose} width={440} aria-label="Commit Changes">
      {open ? <CommitChangesContent project={project} onClose={onClose} /> : null}
    </Sheet>
  )
}

function CommitChangesContent({ project, onClose }: { project: ProjectState; onClose: () => void }) {
  const [committing, setCommitting] = useState(false)
  const message = useProjectValue(project, (p) => p.commitMessage, '')
  const generating = useProjectValue(project, (p) => p.generatingMessage, false)
  const status = useProjectValue(project, (p) => p.gitStatus, null)
  const branch = useProjectValue(project, (p) => p.branch, null)
  const stagedCount = status ? gitStagedFiles(status).length : 0
  const fileCount = status?.files.length ?? 0
  /** Nothing staged: stage everything first. */
  const commitAll = stagedCount === 0
  const canCommit = !committing && !generating && message.trim().length > 0

  const run = () => {
    setCommitting(true)
    const text = project.commitMessage.trim()
    void (async () => {
      try {
        if ((await commit(text, project, { all: commitAll })) !== null) {
          project.update((p) => (p.commitMessage = ''))
          onClose()
        }
      } finally {
        setCommitting(false)
      }
    })()
  }

  const onBranch = branch ?? 'the current branch'
  const explanation: ReactNode = commitAll
    ? `Nothing is staged: all ${fileCount} changed file${fileCount === 1 ? '' : 's'} are staged and committed on ${onBranch}.`
    : `The ${stagedCount} staged file${stagedCount === 1 ? '' : 's'} are committed on ${onBranch}; unstaged changes stay as they are.`

  return (
    <div className={styles.content} onKeyDownCapture={defaultActionKeyDown(run, canCommit)}>
      <div className={styles.title}>Commit Changes</div>
      <div className={styles.tertiaryCaption}>{explanation}</div>
      <GrowingTextEditor
        value={message}
        onChange={(text) => project.update((p) => (p.commitMessage = text))}
        minHeight={72}
        maxHeight={152}
        label="Commit message"
      />
      <div className={styles.buttons}>
        <Button variant="bordered" onClick={onClose}>
          Cancel
        </Button>
        <Tooltip content="Write the message from the changes with the project's model">
          <Button variant="bordered" icon="generate" disabled={generating || committing} onClick={() => void generateCommitMessage(project)}>
            Generate
          </Button>
        </Tooltip>
        {committing || generating ? <BrailleSpinner size={13} /> : null}
        <span className={styles.spacer} />
        <Button variant="prominent" disabled={!canCommit} onClick={run}>
          {commitAll ? 'Commit All' : 'Commit'}
        </Button>
      </div>
    </div>
  )
}
