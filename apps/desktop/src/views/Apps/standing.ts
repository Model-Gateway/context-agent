// Shared state helpers of apps/macos/ContextAgent/Views/Apps/AppsView.swift:
// `AppStanding` (how an app stands right now) and `AppState.standing(of:)`.

import { useMemo } from 'react'
import type { InitializeResult, McpCatalogApp, McpServerStatus } from '../../backend/types'
import { isMac } from '../../components/platform'
import { useApp } from '../../state/app'

/** Computer Use's catalog id: built in, macOS only. */
export const COMPUTER_USE_ID = 'computer-use'

/**
 * `appState.mcpCatalog` as the Apps pages list it. The backend leaves
 * Computer Use out of `mcp/catalog` off macOS (crates/tools/src/mcp/catalog.rs);
 * the app drops it too, so a macOS backend behind a Windows or Linux
 * window (the browser preview) never offers it either.
 */
export function useCatalog(): McpCatalogApp[] {
  const catalog = useApp((state) => state.mcpCatalog)
  return useMemo(() => (isMac ? catalog : catalog.filter((app) => app.id !== COMPUTER_USE_ID)), [catalog])
}

/** `AppStanding`: how an app stands right now, from the configuration and `mcp/status`. */
export type AppStanding =
  | { kind: 'notInstalled' }
  | { kind: 'off' }
  | { kind: 'connecting' }
  | { kind: 'connected'; tools: number }
  | { kind: 'failed'; error: string }
  | { kind: 'installed' }

/** `AppStanding.title`. */
export function standingTitle(standing: AppStanding): string {
  switch (standing.kind) {
    case 'notInstalled':
      return 'Not connected'
    case 'off':
      return 'Paused'
    case 'connecting':
      return 'Connecting'
    case 'connected':
      return 'Connected'
    case 'failed':
      return 'Failed'
    case 'installed':
      return 'Connected'
  }
}

/** `AppStanding.isInstalled`. */
export function standingIsInstalled(standing: AppStanding): boolean {
  return standing.kind !== 'notInstalled'
}

/** `AppStanding ==` (the Swift enum is Equatable). */
export function standingEquals(a: AppStanding, b: AppStanding): boolean {
  if (a.kind !== b.kind) return false
  if (a.kind === 'connected' && b.kind === 'connected') return a.tools === b.tools
  if (a.kind === 'failed' && b.kind === 'failed') return a.error === b.error
  return true
}

/** `AppState.standing(of:)`. */
export function standingOf(config: InitializeResult | null, servers: readonly McpServerStatus[], id: string): AppStanding {
  const server = config?.mcp.servers[id]
  if (!config || !server) return { kind: 'notInstalled' }
  if (!server.enabled || !config.mcp.enabled) return { kind: 'off' }
  const status = servers.find((candidate) => candidate.name === id)
  switch (status?.state) {
    case 'connected':
      return { kind: 'connected', tools: status.tools.filter((tool) => tool.enabled).length }
    case 'connecting':
      return { kind: 'connecting' }
    case 'failed':
      return { kind: 'failed', error: status.error ?? 'unknown error' }
    case 'disabled':
      return { kind: 'off' }
    default:
      return { kind: 'installed' }
  }
}
