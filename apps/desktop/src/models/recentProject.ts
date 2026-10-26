// Port of apps/macos/ContextAgent/Models/RecentProject.swift: remembered
// repository roots for the Welcome grid, the sidebar's pinned projects and
// Open Recent. The Swift `RecentProjectStore` writes
// `recent-projects.json`; here the list is the `recents` preference
// (`ctx.recents` in localStorage, models/preferences.ts), newest first, at
// most 30 entries, pinned ones never dropped.

import { invoke } from '@tauri-apps/api/core'
import { useMemo } from 'react'
import { homeDir } from '@tauri-apps/api/path'
import { getPref, setPref, trimRecentProjects, usePref } from './preferences'

/** `RecentProject`: one remembered repository root. */
export interface RecentProject {
  path: string
  name: string
  lastOpened: Date
  pinned: boolean
}

/** `RecentProject.id`. */
export function recentProjectId(project: RecentProject): string {
  return project.path
}

// MARK: - Paths

/** The home directory, once `homeDir()` has answered (tilde expansion and abbreviation). */
let home: string | null = null
void (async () => {
  try {
    home = stripTrailingSeparators(await homeDir())
  } catch {
    // Outside Tauri: no tilde handling.
  }
})()

function stripTrailingSeparators(path: string): string {
  let out = path
  while (out.length > 1 && (out.endsWith('/') || out.endsWith('\\')) && !/^[A-Za-z]:[\\/]$/.test(out)) out = out.slice(0, -1)
  return out
}

/// `RecentProjectStore.standardize(_:)`: `~` expanded, `.` and `..`
/// segments resolved, repeated and trailing separators dropped. Windows
/// paths keep their backslashes and drive letter.
export function standardizePath(path: string): string {
  let raw = path.trim()
  if (raw.length === 0) return raw
  if (home !== null && (raw === '~' || raw.startsWith('~/') || raw.startsWith('~\\'))) raw = home + raw.slice(1)
  const windows = /^[A-Za-z]:[\\/]/.test(raw) || raw.startsWith('\\\\') || (!raw.includes('/') && raw.includes('\\'))
  const separator = windows ? '\\' : '/'
  let prefix = ''
  let rest = raw
  if (/^[A-Za-z]:[\\/]/.test(raw)) {
    prefix = raw.slice(0, 2) + separator
    rest = raw.slice(3)
  } else if (raw.startsWith('\\\\')) {
    prefix = '\\\\'
    rest = raw.slice(2)
  } else if (raw.startsWith('/') || raw.startsWith('\\')) {
    prefix = separator
    rest = raw.slice(1)
  }
  const parts: string[] = []
  for (const part of rest.split(/[\\/]+/)) {
    if (part.length === 0 || part === '.') continue
    if (part === '..') {
      if (parts.length > 0 && parts[parts.length - 1] !== '..') parts.pop()
      else if (prefix.length === 0) parts.push(part)
      continue
    }
    parts.push(part)
  }
  const joined = prefix + parts.join(separator)
  return joined.length > 0 ? joined : prefix || '.'
}

/** `RecentProject.abbreviatedPath`: "~/Projects/context-agent". */
export function abbreviatedPath(path: string): string {
  if (home === null || home.length <= 1) return path
  if (path === home) return '~'
  if (path.startsWith(home + '/') || path.startsWith(home + '\\')) return '~' + path.slice(home.length)
  return path
}

/** `RecentProject.exists`: the folder is still there (`fs_file_info`). */
export async function recentProjectExists(project: RecentProject): Promise<boolean> {
  try {
    const info = await invoke<{ is_dir: boolean }>('fs_file_info', { path: project.path })
    return info.is_dir
  } catch {
    return false
  }
}

/** The last path component, for a name when the backend gave none. */
export function lastPathComponent(path: string): string {
  const parts = standardizePath(path).split(/[\\/]/).filter((part) => part.length > 0)
  return parts[parts.length - 1] ?? path
}

// MARK: - Ordering

/** `RecentProjectStore.sorted`: pinned first (by last opened), then the rest by last opened. */
export function sortedRecentProjects(items: readonly RecentProject[]): RecentProject[] {
  return [...items].sort((a, b) => {
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1
    return b.lastOpened.getTime() - a.lastOpened.getTime()
  })
}

/** `RecentProjectStore.pinned`. */
export function pinnedRecentProjects(items: readonly RecentProject[]): RecentProject[] {
  return sortedRecentProjects(items).filter((project) => project.pinned)
}

// MARK: - Store

/// `RecentProjectStore`: the operations on the `recents` preference. Every
/// change persists immediately (the Swift store debounced its file write
/// by 200 ms; localStorage writes are cheap).
export const RecentProjects = {
  /** `items`: newest first. */
  items(): RecentProject[] {
    return getPref('recents')
  },

  /** `sorted`. */
  sorted(): RecentProject[] {
    return sortedRecentProjects(getPref('recents'))
  },

  /** `pinned`. */
  pinned(): RecentProject[] {
    return pinnedRecentProjects(getPref('recents'))
  },

  /** `item(for:)`. */
  item(path: string): RecentProject | null {
    return getPref('recents').find((project) => project.path === path) ?? null
  },

  /** `touch(path:name:)`: records an open: moves (or inserts) the entry to the front with the current date. */
  touch(path: string, name: string): void {
    const standardized = standardizePath(path)
    const items = [...getPref('recents')]
    const index = items.findIndex((project) => project.path === standardized)
    const existing = index >= 0 ? items[index] : undefined
    if (existing) {
      items.splice(index, 1)
      items.unshift({ ...existing, lastOpened: new Date(), name })
    } else {
      items.unshift({ path: standardized, name, lastOpened: new Date(), pinned: false })
    }
    setPref('recents', trimRecentProjects(items))
  },

  /** `remove(path:)`. */
  remove(path: string): void {
    const standardized = standardizePath(path)
    setPref(
      'recents',
      getPref('recents').filter((project) => project.path !== standardized),
    )
  },

  /** `setPinned(_:path:)`. */
  setPinned(pinned: boolean, path: string): void {
    const standardized = standardizePath(path)
    const items = getPref('recents')
    const index = items.findIndex((project) => project.path === standardized)
    if (index < 0) return
    setPref(
      'recents',
      items.map((project, i) => (i === index ? { ...project, pinned } : project)),
    )
  },

  /** `togglePin(path:)`. */
  togglePin(path: string): void {
    const standardized = standardizePath(path)
    const entry = RecentProjects.item(standardized)
    if (!entry) return
    RecentProjects.setPinned(!entry.pinned, standardized)
  },

  /** `clearUnpinned()`: Open Recent → Clear Menu drops everything that is not pinned. */
  clearUnpinned(): void {
    setPref(
      'recents',
      getPref('recents').filter((project) => project.pinned),
    )
  },

  /** `clearAll()`. */
  clearAll(): void {
    setPref('recents', [])
  },
}

/** Subscribes a view to the recents, in `sorted` order. */
export function useRecentProjects(): RecentProject[] {
  const items = usePref('recents')
  return useMemo(() => sortedRecentProjects(items), [items])
}
