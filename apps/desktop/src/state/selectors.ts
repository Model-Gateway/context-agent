// The derived values of apps/macos/ContextAgent/App/AppState.swift (its
// computed properties: `selectedProject`, `selectedChat`, `needsAPIKey`,
// `pendingApprovals`, `attentionCount`, `topLevelProjects`, `worktrees(of:)`,
// `connectionLabel`, …) as pure functions of the store's state, plus hooks
// that subscribe a view to them.
//
// The store's objects (`ProjectState`, `ChatState`) are mutated in place and
// every change bumps `version`, so a hook that derives an array or object
// memoises it on `version` (a selector that returned a fresh array on every
// call would make `useSyncExternalStore` loop). Primitive results are
// selected directly.

import { useMemo } from 'react'
import { describe as describeFailure, type BackendFailure } from '../backend/errors'
import { isDiscountDrop, type ApprovalRequest } from '../backend/types'
import { isProjectCollapsed } from '../models/preferences'
import { chatStatusIsRunning } from '../models/reducer'
import { existingBrowser, useApp, type AppStore, type ConnectionState } from './app'
import { useBrowserWorkspace } from './browser'
import type { ChatState } from './chat'
import type { ProjectState } from './project'

/** `GatewayState.credentialName`: the stored credential while signed in to ModelGateway. */
export const GATEWAY_CREDENTIAL = 'modelgateway'

/** The `model.api_key_env` assumed before the backend reported one. */
export const DEFAULT_API_KEY_ENV = 'OPENAI_API_KEY'

/** One pending approval across projects (`AppState.pendingApprovals`' tuple). */
export interface PendingApproval {
  chat: ChatState
  request: ApprovalRequest
}

// MARK: - Connection

/** `ConnectionState.isReady`. */
export function connectionIsReady(connection: ConnectionState): boolean {
  return connection.kind === 'ready'
}

/** `ConnectionState.failure`. */
export function connectionFailure(connection: ConnectionState): BackendFailure | null {
  return connection.kind === 'failed' ? connection.failure : null
}

/** `connection.isReady` of the store. */
export function isReady(state: AppStore): boolean {
  return state.connection.kind === 'ready'
}

/** `connectionLabel`: "ctx 0.1.0 · connected" for the sidebar card. */
export function connectionLabel(state: AppStore): string {
  const connection = state.connection
  switch (connection.kind) {
    case 'connecting':
      return 'Starting backend…'
    case 'ready':
      return `ctx ${connection.result.ctx_version} · connected` + (connection.result.mock ? ' · mock' : '')
    case 'failed':
      return describeFailure(connection.failure).title
  }
}

/// `needsAPIKey`: the configured key variable is unset (not in mock mode).
/// The ModelGateway key comes from sign-in only, so a missing one is never
/// asked for here (the Account page says to sign in again).
export function needsAPIKey(state: AppStore): boolean {
  const config = state.config
  return config?.model.api_key_set === false && config.mock !== true && config.model.credential !== GATEWAY_CREDENTIAL
}

/** `apiKeyEnvName`. */
export function apiKeyEnvName(state: AppStore): string {
  return state.config?.model.api_key_env ?? DEFAULT_API_KEY_ENV
}

// MARK: - Selection

/** `project(for:)`. */
export function projectById(state: AppStore, id: string | null | undefined): ProjectState | null {
  if (id == null) return null
  return state.projects.find((project) => project.id === id) ?? null
}

/** `selectedProject`. */
export function selectedProject(state: AppStore): ProjectState | null {
  return projectById(state, state.selectedProjectId)
}

/** `selectedChat`: the selected session's chat when it exists (never created here). */
export function selectedChat(state: AppStore): ChatState | null {
  const sessionId = state.selectedSessionId
  if (sessionId === null) return null
  return selectedProject(state)?.existingChat(sessionId) ?? null
}

/** The chat of (`projectId`, `sessionId`) when it exists. */
export function chatById(state: AppStore, projectId: string | null | undefined, sessionId: string | null | undefined): ChatState | null {
  if (sessionId == null) return null
  return projectById(state, projectId)?.existingChat(sessionId) ?? null
}

// MARK: - Attention

/** `pendingApprovals`: pending approvals across every open project. */
export function pendingApprovals(state: AppStore): PendingApproval[] {
  const out: PendingApproval[] = []
  for (const project of state.projects) {
    for (const chat of project.pendingApprovalChats) {
      if (chat.pendingApproval) out.push({ chat, request: chat.pendingApproval })
    }
  }
  return out
}

/// `pendingDiscountDrop`: a run asking whether to go on at a much lower
/// ModelGateway discount (the selected chat's question first), shown as an
/// alert over the window; null while its answer is being sent.
export function pendingDiscountDrop(state: AppStore): PendingApproval | null {
  const chat = selectedChat(state)
  const request = chat?.pendingApproval
  if (chat && request && isDiscountDrop(request) && !chat.approvalInFlight) return { chat, request }
  return pendingApprovals(state).find((entry) => isDiscountDrop(entry.request) && !entry.chat.approvalInFlight) ?? null
}

/// `attentionCount`: chats that are unseen or waiting for an approval →
/// the badge (a chat with both counts once).
export function attentionCount(state: AppStore): number {
  return state.projects.reduce((sum, project) => sum + project.attentionCount, 0)
}

/** `anyProjectRunning`. */
export function anyProjectRunning(state: AppStore): boolean {
  return state.projects.some((project) => project.isRunning)
}

/// `canRewind(_:)`: whether the chat can be rewound right now: its session
/// idle and its history loaded (other sessions may run; a rewind restores
/// only this session's files), and the backend ready.
export function canRewind(state: AppStore, chat: ChatState): boolean {
  if (chatStatusIsRunning(chat.status) || chat.status.kind === 'loadingHistory') return false
  const project = projectById(state, chat.projectId)
  if (project && project.isRunningSession(chat.sessionId)) return false
  return isReady(state)
}

// MARK: - Sidebar (worktrees)

/// `topLevelProjects`: projects shown at the top level of the sidebar:
/// everything but the worktrees whose parent checkout is open (those nest under it).
export function topLevelProjects(state: AppStore): ProjectState[] {
  const roots = new Set(state.projects.map((project) => project.root))
  return state.projects.filter((project) => {
    const parent = project.parentRoot
    return parent === null || !roots.has(parent)
  })
}

/** `worktrees(of:)`: the open worktrees of `project`'s checkout. */
export function worktreesOf(state: AppStore, project: ProjectState): ProjectState[] {
  return state.projects.filter((candidate) => candidate.parentRoot === project.root)
}

/** `isCollapsed(_:)`: whether the project's worktree rows are folded away in the sidebar. */
export function isCollapsed(project: ProjectState): boolean {
  return isProjectCollapsed(project.root)
}

/// `visibleWorktrees(of:)`: the worktree rows the sidebar shows under the
/// project: all of them, or only the selected one while it is collapsed.
export function visibleWorktrees(state: AppStore, project: ProjectState): ProjectState[] {
  const all = worktreesOf(state, project)
  if (!isCollapsed(project)) return all
  return all.filter((worktree) => worktree.id === state.selectedProjectId)
}

/** `hiddenWorktrees(of:)`: worktrees a collapsed project hides; their spinner and badge roll up into its row. */
export function hiddenWorktrees(state: AppStore, project: ProjectState): ProjectState[] {
  if (!isCollapsed(project)) return []
  return worktreesOf(state, project).filter((worktree) => worktree.id !== state.selectedProjectId)
}

/** `parentProject(of:)`: the open project of a worktree's parent checkout. */
export function parentProject(state: AppStore, project: ProjectState): ProjectState | null {
  const parent = project.parentRoot
  if (parent === null) return null
  return state.projects.find((candidate) => candidate.root === parent) ?? null
}

/** `pendingProjectPaths`: folders still opening that have no project row yet. */
export function pendingProjectPaths(state: AppStore): string[] {
  const roots = new Set(state.projects.map((project) => project.root))
  return state.openingProjectPaths.filter((path) => !roots.has(path))
}

// MARK: - Hooks

/** Re-renders on every store change; the memo key of the derived hooks. */
export function useAppVersion(): number {
  return useApp((state) => state.version)
}

/** `connection.isReady`. */
export function useIsReady(): boolean {
  return useApp(isReady)
}

/** `connectionLabel`. */
export function useConnectionLabel(): string {
  return useApp(connectionLabel)
}

/** `needsAPIKey`. */
export function useNeedsAPIKey(): boolean {
  return useApp(needsAPIKey)
}

/** `attentionCount`. */
export function useAttentionCount(): number {
  return useApp(attentionCount)
}

/** `anyProjectRunning`. */
export function useAnyProjectRunning(): boolean {
  return useApp(anyProjectRunning)
}

/** `pendingApprovals`, memoised on the store version. */
export function usePendingApprovals(): PendingApproval[] {
  const version = useAppVersion()
  // `version` is the change signal of the store's mutable objects.
  return useMemo(() => pendingApprovals(useApp.getState()), [version])
}

/** `pendingDiscountDrop`, memoised on the store version. */
export function usePendingDiscountDrop(): PendingApproval | null {
  const version = useAppVersion()
  return useMemo(() => pendingDiscountDrop(useApp.getState()), [version])
}

/** `topLevelProjects`, memoised on the store version. */
export function useTopLevelProjects(): ProjectState[] {
  const version = useAppVersion()
  return useMemo(() => topLevelProjects(useApp.getState()), [version])
}

/** `visibleWorktrees(of:)` / `hiddenWorktrees(of:)` of `project`, memoised on the store version. */
export function useWorktreeRows(project: ProjectState | null): { visible: ProjectState[]; hidden: ProjectState[] } {
  const version = useAppVersion()
  return useMemo(() => {
    const state = useApp.getState()
    return project ? { visible: visibleWorktrees(state, project), hidden: hiddenWorktrees(state, project) } : { visible: [], hidden: [] }
  }, [version, project])
}

/** `pendingProjectPaths`, memoised on the store version. */
export function usePendingProjectPaths(): string[] {
  const version = useAppVersion()
  return useMemo(() => pendingProjectPaths(useApp.getState()), [version])
}

/** `canRewind(_:)` for `chat`. */
export function useCanRewind(chat: ChatState | null): boolean {
  return useApp((state) => (chat ? canRewind(state, chat) : false))
}

/// `browserDriven`: whether the agent is acting in the selected project's
/// browser right now (the Browser tab button shows a dot while it is not
/// selected). Follows the workspace's own changes.
export function useBrowserDriven(): boolean {
  const workspace = useApp((state) => existingBrowser(state.selectedProjectId))
  return useBrowserWorkspace(workspace)?.isAgentDriving ?? false
}
