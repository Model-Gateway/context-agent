// Port of apps/macos/ContextAgent/Models/SSHHost.swift: a machine the app
// can run `ctx serve` on over SSH (Settings → Remote hosts), the way Claude
// Code and Codex open sessions on a remote host; `RemoteProjectRef`, the
// `ctx-ssh://<host id>/path` key the preferences and recents use for a
// folder on a host; and `SSHHostStore`, which the Swift app keeps in
// `ssh-hosts.json` next to the recents and this app in the `sshHosts`
// preference (localStorage, next to `recents`). The `Host` aliases of
// `~/.ssh/config` come from the shell (`ssh_config_hosts`).

import { invoke } from '@tauri-apps/api/core'
import { getPref, setPref, usePref } from './preferences'

/// `SSHHost`: `address` is what `ssh` is given (a hostname, an IP, or a
/// `Host` alias from `~/.ssh/config`, which supplies the user, port, key and
/// jumps); the optional fields override that file. `ctxPath` is the remote
/// `ctx` binary, by name on the remote PATH unless given. The field names
/// are the Swift struct's (its JSON is the same shape; the shell's `SshHost`
/// reads it as is).
export interface SSHHost {
  id: string
  /** Shown in the sidebar and the recents; defaults to the address. */
  name: string
  address: string
  user: string
  /** 0 = ssh's default (or the config file's). */
  port: number
  /** `-i`: a private key file ("~/.ssh/id_ed25519"); empty = ssh's choice. */
  identityFile: string
  /** The remote `ctx` binary; empty = "ctx" on the remote PATH (plus the usual install directories). */
  ctxPath: string
  /** Extra `ssh` options, one per entry ("-o", "ProxyJump=bastion"). */
  extraArguments: string[]
}

/** `SSHHost(name:address:)` with a fresh id. */
export function newSSHHost(fields: Partial<Omit<SSHHost, 'id'>> = {}): SSHHost {
  return {
    id: newHostId(),
    name: fields.name ?? '',
    address: fields.address ?? '',
    user: fields.user ?? '',
    port: fields.port ?? 0,
    identityFile: fields.identityFile ?? '',
    ctxPath: fields.ctxPath ?? '',
    extraArguments: fields.extraArguments ?? [],
  }
}

/** A UUID (the Swift ids are UUIDs; any unique string works for the key). */
function newHostId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return crypto.randomUUID().toUpperCase()
  return `${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`.toUpperCase()
}

/** `SSHHost.displayName`. */
export function hostDisplayName(host: SSHHost): string {
  const trimmed = host.name.trim()
  return trimmed.length > 0 ? trimmed : host.address.trim()
}

/** `SSHHost.target`: "alice@devbox:2222", what ssh will connect to, for the forms and tooltips. */
export function hostTarget(host: SSHHost): string {
  const address = host.address.trim()
  const user = host.user.trim()
  let text = user.length > 0 ? `${user}@${address}` : address
  if (host.port > 0) text += `:${host.port}`
  return text
}

/** `SSHHost.isValid`: whether the fields describe something ssh can be given. */
export function hostIsValid(host: SSHHost): boolean {
  const trimmed = host.address.trim()
  return trimmed.length > 0 && !/\s/.test(trimmed) && !trimmed.startsWith('-') && host.port >= 0 && host.port <= 65535
}

/** `SSHHost.normalized()`: trimmed fields, the name defaulted from the address. */
export function normalizeHost(host: SSHHost): SSHHost {
  const address = host.address.trim()
  const name = host.name.trim()
  return {
    id: host.id,
    name: name.length > 0 ? name : address,
    address,
    user: host.user.trim(),
    port: Number.isFinite(host.port) ? Math.max(0, Math.trunc(host.port)) : 0,
    identityFile: host.identityFile.trim(),
    ctxPath: host.ctxPath.trim(),
    extraArguments: host.extraArguments.map((argument) => argument.trim()).filter((argument) => argument.length > 0),
  }
}

/** Two hosts with the same fields (`Equatable`). */
export function sameHost(a: SSHHost, b: SSHHost): boolean {
  return (
    a.id === b.id &&
    a.name === b.name &&
    a.address === b.address &&
    a.user === b.user &&
    a.port === b.port &&
    a.identityFile === b.identityFile &&
    a.ctxPath === b.ctxPath &&
    a.extraArguments.length === b.extraArguments.length &&
    a.extraArguments.every((argument, index) => argument === b.extraArguments[index])
  )
}

// MARK: - RemoteProjectRef

/** `RemoteProjectRef.scheme`. */
export const REMOTE_PROJECT_SCHEME = 'ctx-ssh'

/// `RemoteProjectRef`: a project on a remote host, as the preferences
/// remember it (`ctx-ssh://<host id>/abs/path`): the open-project list and
/// the recents are shared with local folders, which are plain paths.
export interface RemoteProjectRef {
  hostId: string
  path: string
}

/** `RemoteProjectRef.encoded`. */
export function encodeRemoteProject(hostId: string, path: string): string {
  return `${REMOTE_PROJECT_SCHEME}://${hostId}${path.startsWith('/') ? path : '/' + path}`
}

/** `RemoteProjectRef(encoded:)`: null for a local path. */
export function decodeRemoteProject(encoded: string): RemoteProjectRef | null {
  const prefix = `${REMOTE_PROJECT_SCHEME}://`
  if (!encoded.startsWith(prefix)) return null
  const rest = encoded.slice(prefix.length)
  const slash = rest.indexOf('/')
  if (slash <= 0) return null
  return { hostId: rest.slice(0, slash), path: rest.slice(slash) }
}

/** `RemoteProjectRef.isRemote(_:)`. */
export function isRemoteProjectKey(encoded: string): boolean {
  return encoded.startsWith(`${REMOTE_PROJECT_SCHEME}://`)
}

/** `RecentProject.key(path:hostId:)`: the path locally, the encoded ref on a host. */
export function projectKey(path: string, hostId: string | null | undefined): string {
  return hostId == null ? path : encodeRemoteProject(hostId, path)
}

// MARK: - SSHHostStore

/// `SSHHostStore`: the configured hosts, in the order they were added,
/// persisted as the `sshHosts` preference. Every change persists at once.
export const SSHHosts = {
  /** `hosts`. */
  items(): SSHHost[] {
    return getPref('sshHosts')
  },

  /** `host(_:)`. */
  host(id: string | null | undefined): SSHHost | null {
    if (id == null) return null
    return getPref('sshHosts').find((host) => host.id === id) ?? null
  },

  /** `upsert(_:)`: adds, or replaces the host with the same id. */
  upsert(host: SSHHost): SSHHost {
    const normalized = normalizeHost(host)
    const items = [...getPref('sshHosts')]
    const index = items.findIndex((candidate) => candidate.id === normalized.id)
    if (index >= 0) items[index] = normalized
    else items.push(normalized)
    setPref('sshHosts', items)
    return normalized
  },

  /** `remove(_:)`. */
  remove(id: string): void {
    setPref(
      'sshHosts',
      getPref('sshHosts').filter((host) => host.id !== id),
    )
  },
}

/** Subscribes a view to the configured hosts. */
export function useSSHHosts(): SSHHost[] {
  return usePref('sshHosts')
}

/// `SSHConfigHosts.aliases()`: the `Host` aliases of the user's SSH
/// configuration (patterns with wildcards skipped, `Include`s read one level
/// deep), read by the shell; empty outside Tauri.
export async function sshConfigAliases(): Promise<string[]> {
  try {
    const aliases = await invoke<string[]>('ssh_config_hosts')
    return Array.isArray(aliases) ? aliases.filter((alias): alias is string => typeof alias === 'string') : []
  } catch {
    return []
  }
}
