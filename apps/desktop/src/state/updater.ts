// Port of apps/macos/ContextAgent/App/Updater.swift (`AppState.updater`):
// self-updates. The shell's `updater` module (src-tauri/src/updater.rs)
// does the work Sparkle does on macOS: the schedule, the feed, the download,
// the signature check, the settings and installing on quit. This store
// mirrors its state (`updater_status`, the `updater:state` event) and shows
// what Sparkle's own windows show (views/Updates/UpdateSheet.tsx): the
// result of Check for Updates…, an update found while automatic installs are
// off (`updater:found`), the download, and "Install and Relaunch", which asks
// first while a task runs (`Updater.postponeRelaunch`).

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { create } from 'zustand'
import { errorText } from '../backend/errors'
import { showAlert } from '../components/dialogs'
import { toasts } from '../components/toasts'
import { useApp } from './app'
import { anyProjectRunning } from './selectors'

/** A release the feed announced. */
export interface UpdateRelease {
  version: string
  /** Markdown release notes, when the release has them. */
  notes: string | null
  /** RFC 3339. */
  date: string | null
}

export type UpdatePhase =
  | { kind: 'idle' }
  | { kind: 'checking' }
  /** Found, not downloaded. */
  | { kind: 'available'; release: UpdateRelease }
  | { kind: 'downloading'; release: UpdateRelease; received: number; total: number | null }
  /** Downloaded and verified: installs on quit, or now with Install and Relaunch. */
  | { kind: 'ready'; release: UpdateRelease }

/** `updater_status()` / `updater:state`. */
export interface UpdaterStatus {
  /** `Updater.isAvailable`: false for builds that never update themselves (not installed, `--mock`, `--autotest`, macOS and Linux). */
  available: boolean
  /** The running version. */
  version: string
  automatically_checks: boolean
  automatically_installs: boolean
  /** RFC 3339; null before the first check. */
  last_check: string | null
  phase: UpdatePhase
}

/**
 * What the update sheet shows: a check under way, the up-to-date message,
 * the release (available, downloading or ready, from `status.phase`), or
 * the question before a relaunch that would stop a running task.
 */
export type UpdateSheetContent = { kind: 'checking' } | { kind: 'upToDate' } | { kind: 'release' } | { kind: 'postpone' }

interface UpdaterStore {
  status: UpdaterStatus | null
  sheet: UpdateSheetContent | null
}

export const useUpdater = create<UpdaterStore>(() => ({ status: null, sheet: null }))

/** Builds without the shell (the browser preview without its bridge) have no updater. */
function shellAvailable(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

function setStatus(status: UpdaterStatus): void {
  useUpdater.setState((state) => {
    // The release view closes when there is no release left to show (skipped, or a failed check replaced it).
    const releaseGone = state.sheet?.kind === 'release' && (status.phase.kind === 'idle' || status.phase.kind === 'checking')
    return { status, sheet: releaseGone ? null : state.sheet }
  })
}

/** `Updater.canCheckForUpdates`: false while a check or a download is under way. */
export function canCheckForUpdates(status: UpdaterStatus | null): boolean {
  return status !== null && status.available && status.phase.kind !== 'checking' && status.phase.kind !== 'downloading'
}

/** Loads the state and follows the shell's events; MainWindow calls it once. Returns the unsubscribe. */
export function startUpdater(): () => void {
  if (!shellAvailable()) return () => undefined
  const unlisten = [
    listen<UpdaterStatus>('updater:state', (event) => setStatus(event.payload)),
    // A scheduled check found an update while automatic installs are off: offer it, as Sparkle's alert does.
    listen<UpdateRelease>('updater:found', () => {
      if (useUpdater.getState().sheet === null) useUpdater.setState({ sheet: { kind: 'release' } })
    }),
  ]
  void refreshUpdater()
  return () => {
    for (const pending of unlisten) void pending.then((stop) => stop())
  }
}

/** `Updater.refresh()`: Settings calls it when it appears. */
export async function refreshUpdater(): Promise<void> {
  if (!shellAvailable()) return
  try {
    setStatus(await invoke<UpdaterStatus>('updater_status'))
  } catch {
    // An older shell without the command: no updater.
  }
}

/** Bumped by every Check for Updates…, so a check whose sheet was closed meanwhile shows nothing. */
let checkGeneration = 0

/**
 * Check for Updates… / Settings → Check Now: the sheet shows the check, then
 * its result. A download or a downloaded update is shown as it is, without
 * another check.
 */
export async function checkForUpdates(): Promise<void> {
  const status = useUpdater.getState().status
  if (!status?.available) return
  if (status.phase.kind === 'downloading' || status.phase.kind === 'ready') {
    useUpdater.setState({ sheet: { kind: 'release' } })
    return
  }
  const generation = ++checkGeneration
  useUpdater.setState({ sheet: { kind: 'checking' } })
  try {
    const after = await invoke<UpdaterStatus>('updater_check')
    setStatus(after)
    if (generation !== checkGeneration || useUpdater.getState().sheet?.kind !== 'checking') return
    useUpdater.setState({ sheet: after.phase.kind === 'idle' ? { kind: 'upToDate' } : { kind: 'release' } })
  } catch (error) {
    if (generation !== checkGeneration || useUpdater.getState().sheet?.kind !== 'checking') return
    useUpdater.setState({ sheet: null })
    void showAlert({
      title: 'Update Error!',
      message: 'An error occurred in retrieving update information. Please try again later.',
      details: errorText(error),
    })
  }
}

export function closeUpdateSheet(): void {
  checkGeneration += 1
  useUpdater.setState({ sheet: null })
}

/** Remind Me Later (`skip` false) or Skip This Version. */
export async function dismissUpdate(skip: boolean): Promise<void> {
  closeUpdateSheet()
  try {
    setStatus(await invoke<UpdaterStatus>('updater_dismiss', { skip }))
  } catch (error) {
    toasts.push({ message: 'Could not dismiss the update', level: 'error', details: errorText(error) })
  }
}

/** Install Update: downloads with progress in the sheet, which then offers Install and Relaunch. */
export async function downloadUpdate(): Promise<void> {
  try {
    setStatus(await invoke<UpdaterStatus>('updater_download'))
  } catch (error) {
    void showAlert({ title: 'Update Error!', message: 'An error occurred while downloading the update. Please try again later.', details: errorText(error) })
  }
}

/**
 * Install and Relaunch. Restarting stops a running task, so while one runs
 * the sheet asks first (`Updater.postponeRelaunch`); `force` is Restart Now.
 */
export async function installAndRelaunch(force = false): Promise<void> {
  if (!force && anyProjectRunning(useApp.getState())) {
    useUpdater.setState({ sheet: { kind: 'postpone' } })
    return
  }
  closeUpdateSheet()
  try {
    // Exits the app when the installer starts; it opens Context again.
    await invoke('updater_install')
  } catch (error) {
    // The shell stopped the backend for the installer: start it again.
    void useApp.getState().restartBackend()
    void showAlert({ title: 'Update Error!', message: 'The update could not be installed.', details: errorText(error) })
  }
}

/** Restart When Done: installs once no task is running (polled every 2 s, like Swift's loop). */
let waitingForTasks = false

export function installWhenTasksFinish(): void {
  closeUpdateSheet()
  toasts.push({ message: 'Context will restart to finish updating once the running tasks are done.' })
  if (waitingForTasks) return
  waitingForTasks = true
  const poll = () => {
    if (anyProjectRunning(useApp.getState())) {
      window.setTimeout(poll, 2000)
      return
    }
    waitingForTasks = false
    void installAndRelaunch(true)
  }
  poll()
}

/** Settings → Updates → Automatic. */
export async function setAutomaticUpdates(change: { checks?: boolean; installs?: boolean }): Promise<void> {
  try {
    setStatus(await invoke<UpdaterStatus>('updater_set_automatic', change))
  } catch (error) {
    toasts.push({ message: 'Could not change the update settings', level: 'error', details: errorText(error) })
  }
}
