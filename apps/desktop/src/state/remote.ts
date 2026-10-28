// Port of the `RemoteConnection` class of apps/macos/ContextAgent/App/
// RemoteHosts.swift (docs/desktop-protocol.md "Remote hosts"): a host's
// `ssh <host> ctx serve` connection and what the sidebar shows about it.
// The intents (connect, disconnect, reconnect, open a project on a host,
// the remembered remote projects at launch, host CRUD, Test connection)
// live in state/app.ts next to the local backend's, since they share its
// project registration and notification routing. A remote project is an
// ordinary `ProjectState` whose `backend` is this connection's client and
// whose `host` names it; everything else — chats, events, git, the editor
// — works unchanged through the same protocol.

import { BackendClient, looksIncompatible } from '../backend/client'
import { describe as describeFailure, type BackendFailure } from '../backend/errors'
import type { InitializeResult } from '../backend/types'
import { hostDisplayName, hostTarget, type SSHHost } from '../models/sshHost'
import { useUpdater } from './updater'

/** Where Install ctx on host puts the binary (src-tauri/src/backend/install.rs). */
export const INSTALL_DIRECTORY = '~/.local/bin'

/** `RemoteConnection.State`. */
export type RemoteState =
  | { kind: 'disconnected' }
  | { kind: 'connecting' }
  | { kind: 'ready'; result: InitializeResult }
  | { kind: 'failed'; failure: BackendFailure }

/// The remote folder picker sheet (views/Remote/RemoteFolderPicker.tsx):
/// which host it opens on (null: choose one first).
export interface RemotePickerRequest {
  id: number
  host: SSHHost | null
}

/**
 * `RemoteConnection`: one per configured host, created (disconnected) on
 * first use and kept until the host is removed. Mutated in place like
 * `ProjectState`; every change ends with `touch()`, which bumps `version`
 * and tells the app store (state/app.ts wires `RemoteConnection.changeSink`).
 */
export class RemoteConnection {
  /// Called after every `touch()` of any connection; state/app.ts sets it to
  /// bump the store. Null outside the app (tests).
  static changeSink: ((connection: RemoteConnection) => void) | null = null

  /// The host as configured when the connection was made (Settings edits
  /// apply at the next connect).
  host: SSHHost
  readonly client: BackendClient
  state: RemoteState = { kind: 'disconnected' }
  /// Why the last attempt failed, in the user's terms (keys, PATH, the
  /// address), under the raw ssh output.
  hint: string | null = null
  /** A connect in flight, awaited by every opener that arrives meanwhile. */
  connecting: Promise<boolean> | null = null
  /// The last attempt failed because ctx is missing on the host or too
  /// old: Install ctx on host fixes it.
  needsCtx = false
  /** Install ctx on host is running. */
  installing = false
  /** Bumped by every `touch()`. */
  version = 0

  constructor(host: SSHHost, client?: BackendClient) {
    this.host = host
    this.client = client ?? new BackendClient(host.id)
  }

  get id(): string {
    return this.host.id
  }

  get isReady(): boolean {
    return this.state.kind === 'ready'
  }

  /** `state.failure`. */
  get failure(): BackendFailure | null {
    return this.state.kind === 'failed' ? this.state.failure : null
  }

  /** The host's `initialize` result while connected. */
  get result(): InitializeResult | null {
    return this.state.kind === 'ready' ? this.state.result : null
  }

  /** Connected to a ctx older than the app's (Update ctx on host). */
  get isOutdated(): boolean {
    const result = this.result
    const appVersion = useUpdater.getState().status?.version
    return result !== null && !result.mock && appVersion !== undefined && isOlderVersion(result.ctx_version, appVersion)
  }

  /** `displayName` of the host. */
  get displayName(): string {
    return hostDisplayName(this.host)
  }

  /** "ctx 0.1.0 · connected", "Connecting…", "Disconnected", or the failure title. */
  get label(): string {
    switch (this.state.kind) {
      case 'disconnected':
        return 'Disconnected'
      case 'connecting':
        return 'Connecting…'
      case 'ready':
        return `ctx ${this.state.result.ctx_version} · connected` + (this.state.result.mock ? ' · mock' : '')
      case 'failed':
        return describeFailure(this.state.failure).title
    }
  }

  /** "alice@devbox · linux" once connected. */
  get hostSummary(): string | null {
    const info = this.result?.host
    if (!info) return null
    const who = info.user.length === 0 ? info.hostname : `${info.user}@${info.hostname}`
    return `${who} · ${info.os}`
  }

  /** The label with the host summary: what the rows show. */
  get statusText(): string {
    const summary = this.hostSummary
    return summary === null ? this.label : `${this.label} · ${summary}`
  }

  /** Marks the connection changed: bumps `version` and tells the store. */
  touch(): void {
    this.version += 1
    RemoteConnection.changeSink?.(this)
  }

  /** Runs `change` on the connection, then touches it. */
  update(change: (connection: RemoteConnection) => void): void {
    change(this)
    this.touch()
  }
}

/** `RemoteInstaller.isOlder(_:than:)`: "0.1.4" < "0.1.5" by numeric components (after `-`/`+` ignored); false when either is not a version. */
export function isOlderVersion(version: string, other: string): boolean {
  const parts = (text: string): number[] | null => {
    const core = text.split(/[-+]/)[0] ?? ''
    const numbers = core.split('.').map((part) => (/^\d+$/.test(part) ? Number(part) : NaN))
    return numbers.length === 0 || numbers.some(Number.isNaN) ? null : numbers
  }
  const left = parts(version)
  const right = parts(other)
  if (!left || !right) return false
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const a = left[index] ?? 0
    const b = right[index] ?? 0
    if (a !== b) return a < b
  }
  return false
}

/** `AppState.RemoteProblem`: what went wrong reaching a host, read from ssh's last words. */
export type RemoteProblem = 'signIn' | 'unresolved' | 'unreachable' | 'hostKey' | 'ctxMissing' | 'ctxTooOld' | 'sshFailed'

/** `RemoteProblem.isCtx`: Install ctx on host is the fix. */
export function isCtxProblem(problem: RemoteProblem | null): boolean {
  return problem === 'ctxMissing' || problem === 'ctxTooOld'
}

/// `AppState.remoteProblem(for:stderr:)`.
export function remoteProblem(failure: BackendFailure, stderr: readonly string[]): RemoteProblem | null {
  const detail = describeFailure(failure).detail
  const text = (stderr.slice(-12).join('\n') + '\n' + detail).toLowerCase()
  if (text.includes('permission denied') || text.includes('no supported authentication') || text.includes('too many authentication failures')) return 'signIn'
  if (text.includes('could not resolve hostname') || text.includes('name or service not known') || text.includes('nodename nor servname')) return 'unresolved'
  if (
    text.includes('connection refused') ||
    text.includes('connection timed out') ||
    text.includes('operation timed out') ||
    text.includes('no route to host') ||
    text.includes('network is unreachable')
  ) {
    return 'unreachable'
  }
  if (text.includes('host key verification failed') || text.includes('remote host identification has changed')) return 'hostKey'
  if (text.includes('command not found') || (text.includes('not found') && text.includes('ctx')) || (text.includes('no such file or directory') && text.includes('ctx'))) {
    return 'ctxMissing'
  }
  if (failure.kind === 'exited' && failure.code === 127) return 'ctxMissing'
  if (looksIncompatible(text) || failure.kind === 'incompatible') return 'ctxTooOld'
  if (failure.kind === 'exited' && failure.code === 255) return 'sshFailed'
  return null
}

/// `AppState.remoteHint(for:stderr:host:)`: what ssh's last words usually
/// mean, for the sidebar and the toast.
export function remoteHint(failure: BackendFailure, stderr: readonly string[], host: SSHHost): string | null {
  const user = host.user.trim().length === 0 ? 'the configured user' : host.user.trim()
  const name = hostDisplayName(host)
  const target = hostTarget(host)
  switch (remoteProblem(failure, stderr)) {
    case 'signIn':
      return `ssh could not sign in as ${user}. Passwords cannot be typed here: add your key to ssh-agent (ssh-add), set an identity file for the host, or put the host in ~/.ssh/config.`
    case 'unresolved':
      return `The address ${host.address} could not be resolved. Check the host's address, or use a Host alias from ~/.ssh/config.`
    case 'unreachable':
      return `Nothing answered at ${target}. Check that the machine is reachable (VPN, firewall) and that sshd listens on ${host.port > 0 ? `port ${host.port}` : 'its port'}.`
    case 'hostKey':
      return `ssh refused the host key. Connect once from a terminal (ssh ${target}) to accept it, or fix ~/.ssh/known_hosts.`
    case 'ctxMissing':
      return `ctx is not installed on ${name}, or not on its PATH for non-interactive shells. Install ctx on host copies the Linux build of this version to ${INSTALL_DIRECTORY} over ssh (no root or Rust needed); or set the host's ctx path in Settings → Remote hosts.`
    case 'ctxTooOld':
      return `The ctx on ${name} is too old for this app. Update ctx on host copies the Linux build of this version to ${INSTALL_DIRECTORY} over ssh.`
    case 'sshFailed':
      return 'ssh exited with code 255: it could not establish the connection. The output above has the reason; a password prompt, a host-key question or a missing key are the usual ones.'
    case null:
      return null
  }
}

/** `BackendFailurePanel.summary(for:)` for the remote toasts and rows. */
export function remoteFailureSummary(failure: BackendFailure): string {
  return describeFailure(failure).summary
}
