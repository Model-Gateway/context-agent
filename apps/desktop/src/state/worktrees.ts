// Port of apps/macos/ContextAgent/App/WorktreeActions.swift: parallel
// sessions through linked git worktrees (docs/desktop-protocol.md
// "Worktrees"). The backend runs one task per project root, so a second
// prompt in a busy project queues; `worktree/create` gives the repository a
// second checkout on its own `ctx/<name>` branch, opened as a project of its
// own (own run slot, index and chats) and nested under the parent in the
// sidebar. Nothing waits, and the two never edit the same files.

import { backend } from '../backend/client'
import { errorText } from '../backend/errors'
import { worktreeShortMerged, type WorktreeEntry } from '../backend/types'
import { toasts, type ToastLevel } from '../components/toasts'
import { prepareAttachmentPrompt, type ComposerAttachment } from '../models/attachments'
import { forgetOpenProject } from '../models/preferences'
import { existingBrowser, existingTerminals, useApp } from './app'
import type { ChatState } from './chat'
import { loadCommitLog } from './git'
import type { ProjectState } from './project'
import { isReady, parentProject, selectedChat, selectedProject } from './selectors'

/** `ToastCenter.show(_:kind:details:)`. */
function toast(message: string, level: ToastLevel, details?: string): void {
  toasts.push({ message, level, details })
}

/// `newWorktree(in:name:select:)`: creates a worktree of `project`'s
/// repository, opens it and (with `select`, the default) selects its empty
/// chat. `name` becomes the folder and branch name. Resolves with the new
/// worktree's project, null when it could not be created (already toasted).
export async function newWorktree(project: ProjectState, options: { name?: string | null; select?: boolean } = {}): Promise<ProjectState | null> {
  const select = options.select ?? true
  const app = useApp.getState()
  if (!isReady(app)) {
    toast('The backend is not running.', 'error')
    return null
  }
  if (!project.isGitRepository) {
    toast(`${project.name} is not a Git repository, so it cannot have worktrees.`, 'warning')
    return null
  }
  if (project.creatingWorktree) return null
  project.update((p) => (p.creatingWorktree = true))
  try {
    const info = await backend.worktreeCreate(project.id, options.name ?? null)
    const worktree = useApp.getState().register(info)
    await useApp.getState().restorePermissionProfile(worktree)
    if (select) {
      useApp.getState().selectProject(worktree.id, false)
      useApp.getState().newChat()
    }
    useApp.getState().startLoads(worktree)
    // The parent's worktree list and the chip in the sidebar.
    void project.refreshGit()
    return worktree
  } catch (error) {
    const message = errorText(error)
    toast(`Could not create a worktree: ${message}`, 'error', message)
    return null
  } finally {
    project.update((p) => (p.creatingWorktree = false))
  }
}

/// `newWorktreeChat()`: `newWorktree` for the selected project (mod+shift+N,
/// the sidebar, the palette); Welcome when no project is selected.
export function newWorktreeChat(): void {
  const project = selectedProject(useApp.getState())
  if (!project) {
    useApp.getState().setSidebarItem('welcome')
    return
  }
  void newWorktree(project)
}

/// `sendDraftInNewWorktree()`: sends the composer's current draft from a new
/// worktree instead of queueing it behind the running task: the draft (with
/// its attachments) moves to the worktree's first chat and runs there at once.
export async function sendInNewWorktree(): Promise<void> {
  const state = useApp.getState()
  const project = selectedProject(state)
  if (!project) return
  const chat: ChatState | null = selectedChat(state)
  let text: string
  let attachments: ComposerAttachment[]
  if (chat) {
    text = chat.draft
    attachments = chat.attachments
  } else {
    text = state.pendingDraft[project.id] ?? ''
    attachments = state.pendingAttachments[project.id] ?? []
  }
  const trimmed = text.trim()
  if (trimmed.length === 0 && attachments.length === 0) return
  const prompt = await prepareAttachmentPrompt(trimmed, attachments, project.root)
  const worktree = await newWorktree(project, { name: worktreeName(trimmed) })
  if (!worktree) return
  // The draft left the busy project with the prompt.
  if (chat) {
    chat.update((c) => {
      c.draft = ''
      c.attachments = []
    })
  }
  useApp.getState().setPendingDraft(project.id, '')
  useApp.getState().setPendingAttachments(project.id, [])
  if (useApp.getState().selectedProjectId !== worktree.id) return
  await useApp.getState().send(prompt)
}

/** Swift's name for `sendInNewWorktree`. */
export const sendDraftInNewWorktree = sendInNewWorktree

/** `openWorktree(_:)`: opens a worktree of the repository that is not open yet (`worktree/list`). */
export async function openWorktree(entry: WorktreeEntry): Promise<void> {
  await useApp.getState().openProject(entry.path, { select: true, restoreSession: true })
}

/// `removeWorktree(_:deleteBranch:)`: removes a worktree's checkout
/// (uncommitted changes in it are lost) and, on request, its branch.
/// Refused by the backend while a task runs there.
export async function removeWorktree(project: ProjectState, deleteBranch: boolean): Promise<void> {
  if (!project.isWorktree) return
  const parent = parentProject(useApp.getState(), project)
  tearDownWorkspaces(project)
  try {
    const result = await backend.worktreeRemove(project.id, deleteBranch, true)
    forget(project)
    if (parent) {
      if (useApp.getState().selectedProjectId === null) useApp.getState().selectProject(parent.id)
      void parent.refreshGit()
    }
    const branch = result.branch != null ? ` (branch ${result.branch}${result.branch_deleted ? ' deleted' : ' kept'})` : ''
    toast(`Removed worktree ${project.worktreeLabel}${branch}.`, 'info')
  } catch (error) {
    const message = errorText(error)
    toast(`Could not remove the worktree: ${message}`, 'error', message)
  }
}

/// `mergeWorktree(_:deleteAfter:)`: merges the worktree's branch into its
/// parent checkout (a merge commit, `--no-ff`) and, with `deleteAfter`,
/// removes the worktree and its branch. The backend refuses while either
/// project runs, with uncommitted changes in the worktree, and on a conflict
/// (the merge is aborted); the toast carries git's reason.
export async function mergeWorktree(project: ProjectState, deleteAfter: boolean): Promise<void> {
  if (!project.isWorktree) return
  const parent = parentProject(useApp.getState(), project)
  const target = parent?.branch ?? 'the parent branch'
  try {
    const result = await backend.worktreeMerge(project.id, deleteAfter)
    if (result.removed) {
      tearDownWorkspaces(project)
      forget(project)
      if (useApp.getState().selectedProjectId === null && parent) useApp.getState().selectProject(parent.id)
    }
    if (parent) {
      void (async () => {
        await parent.refreshGit()
        await loadCommitLog(parent)
        await parent.refreshStatus()
      })()
    }
    const removed = result.removed ? ' and removed the worktree' : ''
    toast(`Merged ${project.worktreeLabel} into ${target} (${worktreeShortMerged(result)})${removed}.`, 'success', result.subject)
  } catch (error) {
    const message = errorText(error)
    toast(`Could not merge ${project.worktreeLabel} into ${target}: ${message}`, 'error', message)
    if (parent) void parent.refreshGit()
  }
}

/// `worktreeName(for:)`: the first few words of the prompt as the worktree's
/// name; the backend slugifies it and picks a random pair of words when
/// nothing is left. Null when the prompt has no word.
export function worktreeName(prompt: string): string | null {
  const words = prompt
    .split(/\s+/u)
    .map((word) => word.replace(/^\p{P}+|\p{P}+$/gu, ''))
    .filter((word) => word.length > 0 && /[\p{L}\p{M}\p{N}]/u.test(word))
    .slice(0, 4)
  const name = words.join(' ')
  return name.length === 0 ? null : name
}

// MARK: - Helpers

/// `gitRefreshTasks[id]?.cancel()`, `terminals.removeValue(forKey:)?.terminateAll()`,
/// `browsers.removeValue(forKey:)?.tearDown()`. The store keeps the maps
/// private: a pending debounced git refresh finds the project gone and does
/// nothing, and the workspaces are shut down here.
function tearDownWorkspaces(project: ProjectState): void {
  existingTerminals(project.id)?.terminateAll()
  const page = existingBrowser(project.id)
  if (page) void page.tearDown()
}

/// `forget(_:)`: drops a project the backend has already closed
/// (`worktree/remove`, `worktree/merge` with `delete_after`).
function forget(project: ProjectState): void {
  useApp.setState((state) => {
    const selected = state.selectedProjectId === project.id
    return {
      projects: state.projects.filter((candidate) => candidate.id !== project.id),
      ...(selected ? { selectedProjectId: null, selectedSessionId: null } : {}),
      version: state.version + 1,
    }
  })
  forgetOpenProject(project.root)
  project.dispose()
  useApp.getState().refreshAttention()
}
