// Port of apps/macos/ContextAgent/App/GitActions.swift: the Changes tab's git
// actions — stage / unstage / discard, commit (box, per file, amend, undo),
// push / pull / fetch, branches, the log and commit message generation.
// Each action runs one backend `git/*` request under `project.gitBusy`,
// refreshes the status afterwards and reports failures as toasts; the views
// only decide when to ask for confirmation.
//
// `commitFromBox` is plugged into the store's mod+Return (`primaryAction`)
// through `extendApp` when this module loads.

import { backend } from '../backend/client'
import { BackendError, errorText } from '../backend/errors'
import { gitShortHash, gitStagedFiles, type GitCommitResult, type GitDiscardResult } from '../backend/types'
import { toasts, type ToastLevel } from '../components/toasts'
import { extendApp, useApp } from './app'
import { ChatState } from './chat'
import type { ProjectState } from './project'

/** `ToastCenter.show(_:kind:details:)`. */
function toast(message: string, level: ToastLevel, details?: string): void {
  toasts.push({ message, level, details })
}

// MARK: - The action runner

/// `gitAction(_:in:refreshLog:_:)`: runs one git action. Refuses while
/// another is in flight, shows `label` meanwhile (`project.gitBusy`),
/// refreshes the status afterwards (also on failure, since git may have done
/// part of the work) and toasts errors. Resolves with whether `body` succeeded.
export async function gitAction(label: string, project: ProjectState, body: () => Promise<void>, refreshLog = false): Promise<boolean> {
  const busy = project.gitBusy
  if (busy !== null) {
    toast(`Wait for “${busy}” to finish.`, 'info')
    return false
  }
  project.update((p) => (p.gitBusy = label))
  try {
    let ok = true
    try {
      await body()
    } catch (error) {
      ok = false
      const message = errorText(error)
      toast(`${label} failed: ${message}`, 'error', message)
    }
    await project.refreshGit()
    reconcileDiffMode(project)
    if (refreshLog) {
      await loadCommitLog(project)
      await loadBranches(project)
    }
    return ok
  } finally {
    project.update((p) => (p.gitBusy = null))
  }
}

/// `reconcileDiffMode(_:)`: after staging / unstaging the selected file,
/// show the diff that still exists for it (the file moved between the two
/// sections).
function reconcileDiffMode(project: ProjectState): void {
  const path = project.selectedChangePath
  if (path === null) return
  const file = project.gitStatus?.files.find((candidate) => candidate.path === path)
  if (!file) return
  const hasStaged = file.staged
  const hasUnstaged = !file.staged || file.status.length > 1
  if (project.showStaged && !hasStaged && hasUnstaged) {
    void useApp.getState().setShowStaged(false, project)
  } else if (!project.showStaged && hasStaged && !hasUnstaged) {
    void useApp.getState().setShowStaged(true, project)
  }
}

/// `selectChange(path:staged:in:)`: a row click in the Staged or Changes
/// section; the diff follows the section (`showStaged`).
export async function selectChangeInSection(path: string, staged: boolean, project: ProjectState): Promise<void> {
  if (project.showStaged !== staged) project.update((p) => (p.showStaged = staged))
  await useApp.getState().selectChange(path, project)
}

// MARK: - Index

/** `stage(_:in:)`: empty `paths` stages everything. */
export async function stage(paths: string[], project: ProjectState): Promise<void> {
  await gitAction(paths.length === 0 ? 'Staging all changes' : 'Staging', project, async () => {
    await backend.gitStage(project.id, paths)
  })
}

/** `unstage(_:in:)`: empty `paths` unstages everything. */
export async function unstage(paths: string[], project: ProjectState): Promise<void> {
  await gitAction(paths.length === 0 ? 'Unstaging all changes' : 'Unstaging', project, async () => {
    await backend.gitUnstage(project.id, paths)
  })
}

/// `discard(_:in:)`: the views confirm first. Tracked files go back to the
/// index, untracked ones are deleted; empty `paths` discards everything.
export async function discard(paths: string[], project: ProjectState): Promise<void> {
  let result: GitDiscardResult | null = null
  const ok = await gitAction(paths.length === 0 ? 'Discarding all changes' : 'Discarding changes', project, async () => {
    result = await backend.gitDiscard(project.id, paths)
  })
  const discarded = result as GitDiscardResult | null
  if (ok && discarded) {
    const parts: string[] = []
    if (discarded.restored > 0) parts.push(`restored ${discarded.restored} file${discarded.restored === 1 ? '' : 's'}`)
    if (discarded.deleted > 0) parts.push(`deleted ${discarded.deleted} untracked file${discarded.deleted === 1 ? '' : 's'}`)
    toast(parts.length === 0 ? 'Nothing to discard.' : `Discarded: ${parts.join(', ')}.`, 'success')
  }
}

// MARK: - Commits

/** The options of `commit(message:amend:all:paths:in:)`. */
export interface CommitOptions {
  /** `git commit --amend`. */
  amend?: boolean
  /** Stage everything first. */
  all?: boolean
  /** Commit only these files (staged first). */
  paths?: string[]
}

/// `commit(message:amend:all:paths:in:)`: one commit; `paths` commits only
/// those files (staged first), `all` stages everything first. Resolves with
/// the commit on success, null on failure (already toasted).
export async function commit(message: string, project: ProjectState, options: CommitOptions = {}): Promise<GitCommitResult | null> {
  const amend = options.amend ?? false
  const all = options.all ?? false
  const paths = options.paths ?? []
  let result: GitCommitResult | null = null
  const ok = await gitAction(
    amend ? 'Amending' : 'Committing',
    project,
    async () => {
      result = await backend.gitCommit(project.id, message, amend, all, paths)
    },
    true,
  )
  const committed = result as GitCommitResult | null
  if (!ok || !committed) return null
  toast(`${amend ? 'Amended' : 'Committed'} ${gitShortHash(committed)} · ${ChatState.clip(committed.subject, 60)}`, 'success')
  return committed
}

/// `commitFromBox(in:amend:all:)`: the commit box (button, mod+Return).
/// Nothing staged but changes present → asks "stage all and commit?"
/// (`project.askCommitAll`; the Changes view shows it), unless `all`.
export async function commitFromBox(project: ProjectState, options: { amend?: boolean; all?: boolean } = {}): Promise<void> {
  const amend = options.amend ?? false
  const all = options.all ?? false
  const message = project.commitMessage.trim()
  if (message.length === 0) {
    toast('Enter a commit message first.', 'info')
    return
  }
  const status = project.gitStatus
  const nothingStaged = status ? gitStagedFiles(status).length === 0 : true
  if (nothingStaged && !all && !amend) {
    if (!status || status.files.length === 0) {
      toast('Nothing to commit: the working tree is clean.', 'info')
    } else {
      project.update((p) => (p.askCommitAll = true))
    }
    return
  }
  if ((await commit(message, project, { amend, all })) !== null) {
    project.update((p) => (p.commitMessage = ''))
  }
}

/// `undoLastCommit(in:)`: `git reset --soft HEAD~1`; the undone subject
/// becomes the draft when the box is empty.
export async function undoLastCommit(project: ProjectState): Promise<void> {
  let undone: GitCommitResult | null = null
  const ok = await gitAction(
    'Undoing last commit',
    project,
    async () => {
      undone = await backend.gitUndoCommit(project.id)
    },
    true,
  )
  const result = undone as GitCommitResult | null
  if (!ok || !result) return
  if (project.commitMessage.trim().length === 0) {
    project.update((p) => (p.commitMessage = result.subject))
  }
  toast(`Undid ${gitShortHash(result)}; its changes are staged again.`, 'success')
}

/// `generateCommitMessage(in:)`: fills the commit box from the staged
/// changes (or everything when nothing is staged) through the project's
/// model; mock mode and a failed model call produce a heuristic message and
/// say so. Resolves with the message, null when nothing was generated.
export async function generateCommitMessage(project: ProjectState): Promise<string | null> {
  if (project.generatingMessage) return null
  project.update((p) => (p.generatingMessage = true))
  try {
    const generated = await backend.gitCommitMessage(project.id)
    project.update((p) => (p.commitMessage = generated.message))
    if (generated.error != null) {
      toast('The model could not write the message; using a summary of the files instead.', 'warning', generated.error)
    } else if (generated.source === 'heuristic') {
      toast('Mock mode: message summarized from the file list.', 'info')
    }
    return generated.message
  } catch (error) {
    const message = errorText(error)
    toast(`Could not generate a commit message: ${message}`, 'error', message)
    return null
  } finally {
    project.update((p) => (p.generatingMessage = false))
  }
}

// MARK: - Remote

/** The options of `push(remote:branch:setUpstream:force:in:)`. */
export interface PushOptions {
  remote: string | null
  branch: string | null
  setUpstream: boolean
  force: boolean
}

/// `push(remote:branch:setUpstream:force:in:)`: rejects (instead of
/// toasting) so the push sheet can show git's output inline; resolves with
/// git's output.
export async function push(project: ProjectState, options: PushOptions): Promise<string> {
  const busy = project.gitBusy
  if (busy !== null) throw BackendError.rpc(-1, `wait for “${busy}” to finish`)
  project.update((p) => (p.gitBusy = options.force ? 'Force pushing' : 'Pushing'))
  try {
    try {
      const output = await backend.gitPush(project.id, options.remote, options.branch, options.setUpstream, options.force)
      await project.refreshGit()
      await loadBranches(project)
      return output.output
    } catch (error) {
      await project.refreshGit()
      throw error
    }
  } finally {
    project.update((p) => (p.gitBusy = null))
  }
}

/** `pull(rebase:in:)`: the last line of git's output becomes the toast. */
export async function pull(rebase: boolean, project: ProjectState): Promise<void> {
  let output = ''
  const ok = await gitAction(
    rebase ? 'Pulling (rebase)' : 'Pulling',
    project,
    async () => {
      output = (await backend.gitPull(project.id, rebase)).output
    },
    true,
  )
  if (ok) {
    const lines = output.split('\n').filter((line) => line.length > 0)
    const line = lines[lines.length - 1] ?? 'Pulled.'
    toast(ChatState.clip(line, 80), 'success', output.length === 0 ? undefined : output)
  }
}

/** `fetch(in:)`: every remote. */
export async function fetch(project: ProjectState): Promise<void> {
  let output = ''
  const ok = await gitAction(
    'Fetching',
    project,
    async () => {
      output = (await backend.gitFetch(project.id)).output
    },
    true,
  )
  if (ok) toast('Fetched all remotes.', 'success', output.length === 0 ? undefined : output)
}

// MARK: - Branches and log

/** `loadBranches(_:)`: `git/branches` into `project.branches`; failures are ignored. */
export async function loadBranches(project: ProjectState): Promise<void> {
  if (!project.isGitRepository) return
  try {
    const branches = await backend.gitBranches(project.id)
    project.update((p) => (p.branches = branches))
  } catch {
    // `try?` in Swift: the menu keeps what it had.
  }
}

/** `checkout(_:create:in:)`: `git switch [-c]`; refused by the backend while a task runs. */
export async function checkout(branch: string, create: boolean, project: ProjectState): Promise<void> {
  const name = branch.trim()
  if (name.length === 0) return
  let now: string | null = null
  const ok = await gitAction(
    create ? 'Creating branch' : 'Switching branch',
    project,
    async () => {
      now = await backend.gitCheckout(project.id, name, create)
    },
    true,
  )
  if (ok) {
    const current = (now as string | null) ?? name
    toast(create ? `Created and switched to ${current}.` : `Switched to ${current}.`, 'success')
    await project.refreshStatus()
  }
}

/** `loadCommitLog(_:)`: the last 20 commits (`git/log`); failures are ignored. */
export async function loadCommitLog(project: ProjectState): Promise<void> {
  if (!project.isGitRepository) return
  try {
    const log = await backend.gitLog(project.id, 20)
    project.update((p) => (p.commitLog = log))
  } catch {
    // `try?` in Swift: the list keeps what it had.
  }
}

// MARK: - Store wiring

extendApp({ commitFromBox: (project) => commitFromBox(project) })
