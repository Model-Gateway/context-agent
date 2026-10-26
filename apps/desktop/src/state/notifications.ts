// Port of apps/macos/ContextAgent/App/Notifications.swift (`TaskNotifier`):
// system notifications for finished tasks, pending approvals and the agent's
// questions, plus the attention badge; and the window-activity tracking the
// Swift app gets from `NSApp.isActive` / `didBecomeActiveNotification`.
//
// Notifications go through `@tauri-apps/plugin-notification`; permission is
// asked lazily on the first post (requests made meanwhile wait for the
// answer, as `UNUserNotificationCenter` authorisation does). A click is
// reported by the plugin's `onAction` where the platform supports it: the
// window is focused and `onOpen(projectId, sessionId)` selects the chat. The
// badge is `Window.setBadgeCount` (macOS dock, Linux launchers); Windows has
// no numeric taskbar badge, so it is skipped there.

import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  isPermissionGranted,
  onAction,
  removeActive,
  requestPermission,
  sendNotification,
  type Options as NotificationOptions,
} from '@tauri-apps/plugin-notification'
import { platform as osPlatform } from '@tauri-apps/plugin-os'
import { debug } from '../backend/log'
import type { TaskStatus } from '../backend/types'

// MARK: - Environment

/** `TaskNotifier.isAvailable`: only inside the Tauri shell (not in a plain browser preview). */
function tauriAvailable(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

function isWindows(): boolean {
  try {
    return osPlatform() === 'windows'
  } catch {
    return typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
  }
}

// MARK: - Window activity

type ActivityListener = () => void

let active = typeof document === 'undefined' ? true : document.hasFocus()
let activityStarted = false
const activityListeners = new Set<ActivityListener>()

function setActive(next: boolean): void {
  if (next === active) return
  active = next
  if (!next) return
  for (const listener of [...activityListeners]) {
    try {
      listener()
    } catch (error) {
      debug(() => `activity listener threw: ${error instanceof Error ? error.message : String(error)}`)
    }
  }
}

function startActivityTracking(): void {
  if (activityStarted || typeof window === 'undefined') return
  activityStarted = true
  window.addEventListener('focus', () => setActive(true))
  window.addEventListener('blur', () => setActive(document.hasFocus()))
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') setActive(false)
    else setActive(document.hasFocus())
  })
  if (tauriAvailable()) {
    try {
      void getCurrentWindow()
        .onFocusChanged(({ payload: focused }) => setActive(focused))
        .catch(() => undefined)
    } catch {
      // No window handle (tests, previews): the DOM events above suffice.
    }
  }
}

/// `NSApp.isActive`: whether the app's window has the focus. A chat that
/// finishes or asks while the window is in the background is marked unseen
/// and announced.
export function isAppActive(): boolean {
  startActivityTracking()
  return active
}

/// `NSApplication.didBecomeActiveNotification`: `listener` runs each time
/// the window gains the focus. Returns the unsubscribe function.
export function onAppBecameActive(listener: ActivityListener): () => void {
  startActivityTracking()
  activityListeners.add(listener)
  return () => {
    activityListeners.delete(listener)
  }
}

// MARK: - TaskNotifier

type Authorization = 'unknown' | 'requesting' | 'granted' | 'denied'

/** Where a notification points (`userInfo`). */
interface NotificationTarget {
  projectId: string
  sessionId: string
}

/** `TaskNotifier.Category`. */
export const NotificationCategory = {
  taskFinished: 'dev.contextagent.task-finished',
  approval: 'dev.contextagent.approval',
} as const

/// `TaskNotifier`: posts "Task completed · <chat title>", "Approval needed:
/// <summary>" and "Question: <question>", keeps the badge, and routes a
/// click back to the chat through `onOpen`.
export class TaskNotifier {
  /** Set by AppState: selects the project + session the notification points at. */
  onOpen: ((projectId: string, sessionId: string) => void) | null = null

  private authorization: Authorization = 'unknown'
  private queued: NotificationOptions[] = []
  /** Notification id → its chat, for clicks the platform reports without `extra`. */
  private targets = new Map<number, NotificationTarget>()
  private listening = false
  private lastBadge: number | null = null

  // MARK: Posting

  /** "Task completed · <chat title>" (or failed / blocked / cancelled); the error is the body. */
  notifyTaskFinished(status: TaskStatus | null, error: string | null, chatTitle: string, projectId: string, sessionId: string): void {
    let word: string
    switch (status) {
      case 'completed':
        word = 'completed'
        break
      case 'blocked':
        word = 'blocked'
        break
      case 'cancelled':
        word = 'cancelled'
        break
      default:
        word = 'failed'
    }
    this.post(`Task ${word} · ${chatTitle}`, error ?? '', NotificationCategory.taskFinished, projectId, sessionId, `task-${sessionId}`)
  }

  /** "Approval needed: <summary>". */
  notifyApprovalNeeded(summary: string, chatTitle: string, projectId: string, sessionId: string): void {
    this.post(`Approval needed: ${summary}`, chatTitle, NotificationCategory.approval, projectId, sessionId, `approval-${sessionId}`)
  }

  /** "Question: <first question>" from the agent's `ask_user` tool. */
  notifyQuestionAsked(question: string, chatTitle: string, projectId: string, sessionId: string): void {
    this.post(`Question: ${question}`, chatTitle, NotificationCategory.approval, projectId, sessionId, `question-${sessionId}`)
  }

  /** Badge: unseen chats + pending approvals (dock tile on macOS; skipped on Windows). */
  updateBadge(count: number): void {
    if (count === this.lastBadge || !tauriAvailable() || isWindows()) return
    this.lastBadge = count
    try {
      void getCurrentWindow()
        .setBadgeCount(count > 0 ? count : undefined)
        .catch((error: unknown) => debug(() => `setBadgeCount failed: ${String(error)}`))
    } catch (error) {
      debug(() => `setBadgeCount failed: ${String(error)}`)
    }
  }

  /** Removes delivered notifications for a session once the user looked at it. */
  clearDelivered(sessionId: string): void {
    if (!tauriAvailable()) return
    const ids = [`task-${sessionId}`, `approval-${sessionId}`, `question-${sessionId}`].map(notificationId)
    for (const id of ids) this.targets.delete(id)
    void removeActive(ids.map((id) => ({ id }))).catch(() => undefined)
  }

  // MARK: Private

  private post(title: string, body: string, category: string, projectId: string, sessionId: string, identifier: string): void {
    if (!tauriAvailable()) return
    const id = notificationId(identifier)
    const options: NotificationOptions = {
      id,
      title,
      group: category,
      extra: { projectId, sessionId, category },
    }
    if (body.length > 0) options.body = Array.from(body).slice(0, 200).join('')
    this.targets.set(id, { projectId, sessionId })
    this.listenForClicks()

    switch (this.authorization) {
      case 'granted':
        this.deliver(options)
        return
      case 'denied':
        return
      case 'requesting':
        this.queued.push(options)
        return
      case 'unknown':
        this.queued.push(options)
        this.requestAuthorization()
        return
    }
  }

  private requestAuthorization(): void {
    this.authorization = 'requesting'
    void (async () => {
      let granted = false
      try {
        granted = await isPermissionGranted()
        if (!granted) granted = (await requestPermission()) === 'granted'
      } catch {
        granted = false
      }
      this.finishAuthorization(granted)
    })()
  }

  private finishAuthorization(granted: boolean): void {
    this.authorization = granted ? 'granted' : 'denied'
    const queued = this.queued
    this.queued = []
    if (!granted) return
    for (const options of queued) this.deliver(options)
  }

  private deliver(options: NotificationOptions): void {
    try {
      sendNotification(options)
    } catch (error) {
      debug(() => `notification not delivered: ${error instanceof Error ? error.message : String(error)}`)
    }
  }

  private listenForClicks(): void {
    if (this.listening) return
    this.listening = true
    void onAction((event) => {
      const notification = event.notification
      const extra = notification?.extra ?? {}
      const projectId = typeof extra.projectId === 'string' ? extra.projectId : null
      const sessionId = typeof extra.sessionId === 'string' ? extra.sessionId : null
      const target = projectId !== null && sessionId !== null ? { projectId, sessionId } : notification ? this.targets.get(notification.id) : undefined
      this.open(target ?? null)
    }).catch((error: unknown) => {
      debug(() => `notification clicks unavailable: ${String(error)}`)
    })
  }

  private open(target: NotificationTarget | null): void {
    if (!target) return
    try {
      void getCurrentWindow()
        .setFocus()
        .catch(() => undefined)
    } catch {
      // No window handle.
    }
    this.onOpen?.(target.projectId, target.sessionId)
  }
}

/// A stable 31-bit id for a string identifier (`UNNotificationRequest`'s
/// identifier: a newer notification for the same chat replaces the older).
function notificationId(identifier: string): number {
  let hash = 0x811c9dc5
  for (let i = 0; i < identifier.length; i += 1) {
    hash ^= identifier.charCodeAt(i)
    hash = Math.imul(hash, 0x01000193)
  }
  return (hash >>> 1) || 1
}

/** The app's notifier (`AppState.notifier`). */
export const notifier = new TaskNotifier()
