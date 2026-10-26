// Port of apps/macos/ContextAgent/App/AppCommands.swift: every keyboard
// shortcut of the app (apps/macos/ARCHITECTURE.md "Keyboard shortcuts") as
// one global `keydown` handler, plus the menu-only commands the command
// palette reuses (Copy Transcript, Export Chat as Markdown, Find in Chat).
// This module is the only owner of global key handling: views handle keys
// only inside their own focused controls.
//
// `mod` is ⌘ on macOS and Ctrl on Windows and Linux (`isModKey`); the
// sidebar toggle is ⌃⌘S on macOS and Ctrl+Alt+S elsewhere. The handler
// listens on `window` in the bubbling phase and skips events a control has
// already handled (`defaultPrevented`): the overlays (popovers, menus,
// sheets) stop Esc and Return before it, CodeMirror's own keymap (mod+S,
// mod+F inside the editor) prevents the default, and IME composition never
// reaches a command.

import { invoke } from '@tauri-apps/api/core'
import { errorText } from '../backend/errors'
import { isMac, isModKey } from '../components/platform'
import { toasts } from '../components/toasts'
import { RecentProjects } from '../models/recentProject'
import { useApp, type SidebarItem } from './app'
import { closeSelectedDocument, saveSelectedDocument } from './editor'
import { selectedChat, selectedProject } from './selectors'
import { newWorktreeChat, sendInNewWorktree } from './worktrees'

// MARK: - Find in chat

/// The event `findInChat` dispatches on `window` (Swift posts
/// `ChatView.findNotification`); the visible chat opens its find bar.
export const FIND_IN_CHAT_EVENT = 'ctx:find-in-chat'

/** Subscribes to mod+F / the palette's "Find in Chat"; returns the unsubscribe function. */
export function onFindInChat(listener: () => void): () => void {
  const handler = () => listener()
  window.addEventListener(FIND_IN_CHAT_EVENT, handler)
  return () => window.removeEventListener(FIND_IN_CHAT_EVENT, handler)
}

// MARK: - Commands (`AppCommands`' actions)

/// `AppCommands.showTab(_:appState:)`: mod+1 … mod+4. A tab needs an open
/// project; without one the shortcut lands on Welcome.
export function showTab(item: SidebarItem): void {
  const state = useApp.getState()
  if (!selectedProject(state)) {
    state.setSidebarItem('welcome')
    return
  }
  state.setSidebarItem(item)
  if (item === 'chat') state.focusComposer()
}

/// `AppCommands.findInChat(appState:)`: mod+F. Switches to the Chat tab
/// when needed; the chat view opens its find bar on `FIND_IN_CHAT_EVENT`.
export function findInChat(): void {
  const state = useApp.getState()
  if (!selectedChat(state)) return
  const dispatch = () => window.dispatchEvent(new CustomEvent(FIND_IN_CHAT_EVENT))
  if (state.sidebarItem === 'chat') {
    dispatch()
    return
  }
  state.setSidebarItem('chat')
  // The chat view mounts with the next render; tell it once it is there.
  requestAnimationFrame(() => window.setTimeout(dispatch, 0))
}

/** `AppCommands.copyTranscript(appState:)`: mod+shift+C, the whole transcript as Markdown. */
export function copyTranscript(): void {
  const state = useApp.getState()
  const chat = selectedChat(state)
  if (!chat) {
    toasts.push({ message: 'Select a chat to copy its transcript.', level: 'info' })
    return
  }
  state.copyToPasteboard(chat.transcript.markdown(chat.title))
  toasts.push({ message: 'Transcript copied as Markdown', level: 'success' })
}

/// `AppCommands.exportChatAsMarkdown(appState:)`: File → Export Chat as
/// Markdown… (the palette on Windows): the shell's save dialog, then the
/// transcript as Markdown into the chosen file (`fs_save_text`, the only
/// command that writes where the user chooses, so the file may live outside
/// the project).
export async function exportChatAsMarkdown(): Promise<void> {
  const chat = selectedChat(useApp.getState())
  if (!chat) {
    toasts.push({ message: 'Select a chat to export it.', level: 'info' })
    return
  }
  const content = chat.transcript.markdown(chat.title)
  try {
    const path = await invoke<string | null>('fs_save_text', {
      title: 'Export Chat as Markdown',
      default_name: `${exportFileName(chat.title)}.md`,
      content,
      extension: 'md',
    })
    if (!path) return
    const name = path.split(/[\\/]/).pop() ?? path
    toasts.push({ message: `Exported “${name}”`, level: 'success' })
  } catch (error) {
    const message = errorText(error)
    toasts.push({ message: `Could not export the chat: ${message}`, level: 'error', details: message })
  }
}

/// `AppCommands.fileName(for:)`: "Fix spacing on cards" → "Fix spacing on
/// cards"; path separators and control characters are dropped, long titles
/// clipped to 60 characters.
export function exportFileName(title: string): string {
  // eslint-disable-next-line no-control-regex
  let name = title.replace(/[/:\\\u0000-\u001f\u007f-\u009f\u2028\u2029]/g, '').trim()
  if ([...name].length > 60) name = [...name].slice(0, 60).join('').trim()
  return name.length === 0 ? 'Chat' : name
}

/// File → Open Recent → Clear Menu: forgets every recent project that is
/// not pinned.
export function clearRecentProjects(): void {
  RecentProjects.clearUnpinned()
}

/// mod+Backspace denies the visible approval. With nothing to deny the key
/// keeps its text-editing meaning (delete to the line start) instead of
/// being swallowed.
function denyIfPending(): boolean {
  const chat = selectedChat(useApp.getState())
  if (!chat || !chat.pendingApproval || chat.approvalInFlight) return false
  useApp.getState().denyPendingApproval()
  return true
}

// MARK: - Shortcut table

interface Shortcut {
  mod?: boolean
  shift?: boolean
  alt?: boolean
  ctrl?: boolean
  /** Normalised key (`eventKey`). */
  key: string
  /** Held-down repeats are ignored (toggles would flicker). */
  noRepeat?: boolean
  /** Runs the command; `false` lets the event through untouched. */
  run: () => boolean | void
}

/** The sidebar toggle: ⌃⌘S on macOS, Ctrl+Alt+S on Windows and Linux. */
const SIDEBAR_SHORTCUT: Pick<Shortcut, 'mod' | 'ctrl' | 'alt'> = isMac ? { mod: true, ctrl: true } : { mod: true, alt: true }

function app() {
  return useApp.getState()
}

const SHORTCUTS: readonly Shortcut[] = [
  // App menu: Settings… (the profile page's last settings tab).
  { mod: true, key: ',', run: () => app().openSettings() },
  // File
  { mod: true, key: 'n', run: () => app().newChat() },
  { mod: true, shift: true, key: 'n', run: () => newWorktreeChat() },
  { mod: true, key: 'o', run: () => void app().chooseProject() },
  { mod: true, key: 's', run: () => saveSelectedDocument() },
  { mod: true, shift: true, key: 'w', run: () => closeSelectedDocument() },
  // Edit
  { mod: true, shift: true, key: 'c', run: () => copyTranscript() },
  { mod: true, key: 'f', run: () => findInChat() },
  // Chat
  { mod: true, key: 'enter', run: () => app().primaryAction() },
  { mod: true, shift: true, key: 'enter', run: () => void sendInNewWorktree() },
  { mod: true, key: 'backspace', run: () => denyIfPending() },
  { mod: true, key: '.', run: () => void app().cancelRunningTask() },
  { mod: true, alt: true, key: 'z', run: () => app().undoLastTurn() },
  { mod: true, shift: true, key: 'm', noRepeat: true, run: () => app().toggleModelPicker() },
  { mod: true, shift: true, key: 'r', run: () => void app().rebuildIndex() },
  // Go
  { mod: true, key: '1', run: () => showTab('chat') },
  { mod: true, key: '2', run: () => showTab('changes') },
  { mod: true, key: '3', run: () => showTab('project') },
  { mod: true, key: '4', run: () => showTab('editor') },
  { mod: true, key: 'k', noRepeat: true, run: () => app().setPaletteVisible(!app().paletteVisible) },
  // View
  { ...SIDEBAR_SHORTCUT, key: 's', noRepeat: true, run: () => app().toggleSidebar() },
  { mod: true, shift: true, key: 'i', noRepeat: true, run: () => app().toggleInspector() },
]

/// The key of an event, lower-cased: letters and digits by their character
/// (so non-QWERTY layouts keep their letters), falling back to the physical
/// key when Option / Alt turned the character into a symbol (⌥Z → "Ω").
function eventKey(event: KeyboardEvent): string {
  const { code, key } = event
  if (/^Key[A-Z]$/.test(code) && (event.altKey || !/^[a-z]$/i.test(key))) return code.slice(3).toLowerCase()
  if (/^Digit[0-9]$/.test(code) && !/^[0-9]$/.test(key)) return code.slice(5)
  if (/^Numpad[0-9]$/.test(code) && !/^[0-9]$/.test(key)) return code.slice(6)
  if (code === 'NumpadEnter') return 'enter'
  return key.toLowerCase()
}

/** Whether the event's modifiers are exactly the shortcut's. */
function modifiersMatch(event: KeyboardEvent, shortcut: Shortcut): boolean {
  const mod = isModKey(event)
  // On Windows and Linux Ctrl is `mod`; on macOS it is its own modifier.
  const ctrl = isMac ? event.ctrlKey : false
  const meta = isMac ? false : event.metaKey
  return mod === (shortcut.mod ?? false) && event.shiftKey === (shortcut.shift ?? false) && event.altKey === (shortcut.alt ?? false) && ctrl === (shortcut.ctrl ?? false) && !meta
}

/// Esc (no modifiers): the command palette, then the model picker, then an
/// inline message edit. Esc never answers an approval; anything else keeps
/// the event (the composer clears its own selection).
function handleEscape(): boolean {
  const state = app()
  if (state.paletteVisible) {
    state.setPaletteVisible(false)
    state.focusComposer()
    return true
  }
  if (state.modelPickerVisible) {
    state.setModelPickerVisible(false)
    state.focusComposer()
    return true
  }
  const chat = selectedChat(state)
  if (state.rewindRequest === null && chat && chat.editingTurnId !== null) {
    state.cancelEdit(chat)
    return true
  }
  return false
}

/** A modal sheet (alert, confirmation, rewind) is up: the window's commands wait for it. */
function modalOpen(): boolean {
  return document.querySelector('[aria-modal="true"]') !== null
}

/** The one global `keydown` handler. */
function onKeyDown(event: KeyboardEvent): void {
  if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return
  if (event.key === 'Escape') {
    if (event.shiftKey || event.altKey || event.ctrlKey || event.metaKey) return
    if (handleEscape()) event.preventDefault()
    return
  }
  if (!isModKey(event) || modalOpen()) return
  const key = eventKey(event)
  const shortcut = SHORTCUTS.find((candidate) => candidate.key === key && modifiersMatch(event, candidate))
  if (!shortcut) return
  if (event.repeat && shortcut.noRepeat) {
    event.preventDefault()
    return
  }
  if (shortcut.run() === false) return
  event.preventDefault()
  event.stopPropagation()
}

let installed = 0

/// `AppCommands.installKeyMonitor(appState:)` and the menu's key
/// equivalents: installs the global handler (once, however often it is
/// called) and returns the function that removes it.
export function installGlobalKeys(): () => void {
  installed += 1
  if (installed === 1) window.addEventListener('keydown', onKeyDown)
  let removed = false
  return () => {
    if (removed) return
    removed = true
    installed -= 1
    if (installed === 0) window.removeEventListener('keydown', onKeyDown)
  }
}
