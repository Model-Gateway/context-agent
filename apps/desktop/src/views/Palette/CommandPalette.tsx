// Port of apps/macos/ContextAgent/Views/Palette/CommandPalette.swift: the
// mod+K command palette, a floating 560 px panel over the window with a
// search field, "Chats" (titles plus the project's task objectives /
// summaries from `ProjectState.tasks`, matched snippet, relative time) and
// "Actions" (New chat, New chat in a worktree, Open project…, Switch model…,
// Rebuild index, Toggle inspector, Run doctor, Settings…). ↑↓ move, Return
// activates, Esc closes; Esc and Return never reach the composer.
//
// Windows has no menu bar (and the macOS build keeps Tauri's default menu),
// so the commands the Swift app offers only in its menus follow Swift's
// eight actions here: Find in chat, Copy transcript as Markdown, Export chat
// as Markdown… (File), Toggle auto save (File → Auto Save), Toggle sidebar
// (View), Go to Welcome / Projects / Apps (Go), one "Open <name>" per recent
// project and Clear recent projects (File → Open Recent). They run the same
// commands as the shortcuts and menus (state/keys.ts).

import { memo, useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type MouseEvent } from 'react'
import { createPortal } from 'react-dom'
import { sessionUpdatedDate, type TaskSummary } from '../../backend/types'
import { Chip } from '../../components/Chip'
import { Kbd } from '../../components/Kbd'
import { useFocusTrap, useOverlay } from '../../components/overlays'
import { isMac } from '../../components/platform'
import { ModelCatalog } from '../../models/modelCatalog'
import { getPref, usePref } from '../../models/preferences'
import { abbreviatedPath, recentProjectId, sortedRecentProjects, type RecentProject } from '../../models/recentProject'
import { TranscriptFormat } from '../../models/transcript'
import { useApp } from '../../state/app'
import { ChatState } from '../../state/chat'
import { setEditorAutoSave } from '../../state/editor'
import { clearRecentProjects, copyTranscript, exportChatAsMarkdown, findInChat } from '../../state/keys'
import { selectedChat, useAppVersion } from '../../state/selectors'
import { newWorktreeChat } from '../../state/worktrees'
import { Icon, type IconName } from '../../theme/icons'
import styles from './CommandPalette.module.css'

/** `.transition(.opacity)` with `.easeOut(duration: 0.15)` on show and hide. */
const FADE_MS = 150

/**
 * `CommandPalette` (Views/Palette/CommandPalette.swift): shown while
 * `paletteVisible`, over the whole window (portalled into the document
 * body). Each opening starts with an empty query.
 */
export function CommandPalette() {
  const visible = useApp((state) => state.paletteVisible)
  const [mounted, setMounted] = useState(visible)
  const [leaving, setLeaving] = useState(false)
  /** Bumped by every opening, so a palette reopened while it fades out starts afresh. */
  const [generation, setGeneration] = useState(0)
  const wasVisible = useRef(visible)

  useEffect(() => {
    const opened = visible && !wasVisible.current
    wasVisible.current = visible
    if (visible) {
      if (opened) setGeneration((value) => value + 1)
      setMounted(true)
      setLeaving(false)
      return
    }
    if (!mounted) return
    setLeaving(true)
    const timer = window.setTimeout(() => {
      setMounted(false)
      setLeaving(false)
    }, FADE_MS)
    return () => window.clearTimeout(timer)
  }, [visible, mounted])

  if (!mounted || typeof document === 'undefined') return null
  return createPortal(<PaletteOverlay key={generation} active={visible && !leaving} leaving={leaving} />, document.body)
}

// MARK: - Rows

/** `PaletteAction`, then the menu-only commands (see the file comment). */
type PaletteAction =
  | 'newChat'
  | 'newWorktree'
  | 'openProject'
  | 'openRemoteProject'
  | 'switchModel'
  | 'rebuildIndex'
  | 'toggleInspector'
  | 'runDoctor'
  | 'settings'
  | 'findInChat'
  | 'copyTranscript'
  | 'exportChat'
  | 'toggleAutoSave'
  | 'toggleSidebar'
  | 'goWelcome'
  | 'goProjects'
  | 'goApps'

const PALETTE_ACTIONS: readonly PaletteAction[] = [
  'newChat',
  'newWorktree',
  'openProject',
  'openRemoteProject',
  'switchModel',
  'rebuildIndex',
  'toggleInspector',
  'runDoctor',
  'settings',
  'findInChat',
  'copyTranscript',
  'exportChat',
  'toggleAutoSave',
  'toggleSidebar',
  'goWelcome',
  'goProjects',
  'goApps',
]

interface ActionInfo {
  title: string
  detail: string
  /** Spelled with `mod` for `Kbd` (Ctrl+Shift+N off macOS). */
  shortcut: string | null
  /** The Swift string verbatim ("⌘⇧N"), shown on macOS. */
  macShortcut: string | null
  icon: IconName
  needsProject: boolean
  /** Needs a selected chat (the Swift commands toast "Select a chat…" without one). */
  needsChat?: boolean
}

const ACTION_INFO: Record<PaletteAction, ActionInfo> = {
  newChat: { title: 'New chat', detail: 'Start an empty chat in the selected project', shortcut: 'mod+N', macShortcut: '⌘N', icon: 'newChat', needsProject: true },
  newWorktree: { title: 'New chat in a worktree', detail: 'A second checkout of the repository, so tasks run side by side', shortcut: 'mod+shift+N', macShortcut: '⌘⇧N', icon: 'worktree', needsProject: true },
  openProject: { title: 'Open project…', detail: 'Choose a repository folder', shortcut: 'mod+O', macShortcut: '⌘O', icon: 'projects', needsProject: false },
  openRemoteProject: { title: 'Open project on remote host…', detail: 'Browse a folder on an SSH host configured in Settings', shortcut: 'mod+shift+O', macShortcut: '⌘⇧O', icon: 'remoteHost', needsProject: false },
  switchModel: { title: 'Switch model…', detail: "Pick the project's model", shortcut: 'mod+shift+M', macShortcut: '⌘⇧M', icon: 'model', needsProject: true },
  rebuildIndex: { title: 'Rebuild index', detail: 'Re-index the repository from scratch', shortcut: 'mod+shift+R', macShortcut: '⌘⇧R', icon: 'refresh', needsProject: true },
  toggleInspector: { title: 'Toggle inspector', detail: 'Show or hide the Run / Log pane', shortcut: 'mod+shift+I', macShortcut: '⌘⇧I', icon: 'inspector', needsProject: false },
  runDoctor: { title: 'Run doctor', detail: 'Check configuration, repository and provider', shortcut: null, macShortcut: null, icon: 'doctor', needsProject: true },
  settings: { title: 'Settings…', detail: 'General, Model, Permissions, Pricing, Appearance', shortcut: 'mod+,', macShortcut: '⌘,', icon: 'settings', needsProject: false },
  findInChat: { title: 'Find in chat', detail: 'Search the messages of the selected chat', shortcut: 'mod+F', macShortcut: '⌘F', icon: 'search', needsProject: true, needsChat: true },
  copyTranscript: { title: 'Copy transcript as Markdown', detail: 'The whole chat on the clipboard', shortcut: 'mod+shift+C', macShortcut: '⌘⇧C', icon: 'copy', needsProject: true, needsChat: true },
  exportChat: { title: 'Export chat as Markdown…', detail: 'Save the whole chat as a Markdown file', shortcut: null, macShortcut: null, icon: 'docDown', needsProject: true, needsChat: true },
  toggleAutoSave: { title: 'Toggle auto save', detail: 'Write an edited file a second after you stop typing', shortcut: null, macShortcut: null, icon: 'save', needsProject: false },
  toggleSidebar: { title: 'Toggle sidebar', detail: 'Show or hide the projects and chats', shortcut: isMac ? 'mod+ctrl+S' : 'mod+alt+S', macShortcut: '⌃⌘S', icon: 'sidebarLeft', needsProject: false },
  goWelcome: { title: 'Go to Welcome', detail: 'The start page with your recent projects', shortcut: null, macShortcut: null, icon: 'welcome', needsProject: false },
  goProjects: { title: 'Go to Projects', detail: 'Every project you have opened', shortcut: null, macShortcut: null, icon: 'projects', needsProject: false },
  goApps: { title: 'Go to Apps', detail: 'Connect apps and MCP servers', shortcut: null, macShortcut: null, icon: 'apps', needsProject: false },
}

/** Recent projects listed while the query is empty; a query searches all of them. */
const RECENTS_SHOWN = 5

type PaletteSection = 'chats' | 'actions'

function sectionTitle(section: PaletteSection): string {
  return section === 'chats' ? 'Chats' : 'Actions'
}

type PaletteKind =
  | { kind: 'chat'; projectId: string; sessionId: string }
  | { kind: 'action'; action: PaletteAction }
  /** File → Open Recent → <name>. */
  | { kind: 'recent'; recent: RecentProject }
  /** File → Open Recent → Clear Menu. */
  | { kind: 'clearRecents' }

/** `PaletteRow`. */
interface PaletteRow {
  id: string
  section: PaletteSection
  title: string
  snippet: string | null
  /** A shortcut (`Kbd`) for actions, plain caption text otherwise. */
  trailing: string | null
  /** Swift's verbatim shortcut text, shown on macOS instead of `trailing`'s formatted one. */
  macTrailing?: string | null
  /** Plain caption text at the right of an action ("On" / "Off"). */
  trailingText?: string | null
  subtitle: string | null
  icon: IconName
  kind: PaletteKind
}

// MARK: - Overlay

/** `results.count * 46 + sections * 26 + 16`, up to 420: the list hugs its rows, then scrolls. */
function listHeight(results: PaletteRow[]): number {
  const sections = new Set(results.map((row) => row.section)).size
  return Math.min(420, results.length * 46 + sections * 26 + 16)
}

/// The dimmed window and the panel. `active` is false while it fades out:
/// it no longer takes keys, focus or clicks then.
function PaletteOverlay({ active, leaving }: { active: boolean; leaving: boolean }) {
  const version = useAppVersion()
  const [query, setQuery] = useState('')
  const [selection, setSelection] = useState(0)
  /** Bumped by ↑↓ and typing; only these scroll the selection into view (hover selects without scrolling). */
  const [scrollRequest, setScrollRequest] = useState(0)
  /** Screen location of the last hover that changed the selection; a hover at the same spot comes from rows scrolling under a still cursor. */
  const lastHoverLocation = useRef<{ x: number; y: number } | null>(null)
  const overlayRef = useRef<HTMLDivElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const listId = useId()

  // Every store change re-runs the search (`version` is the store's change
  // signal); the recents and auto save live in the preferences.
  const recents = usePref('recents')
  const autoSave = usePref('editorAutoSave')
  const results = useMemo(() => paletteResults(query), [query, version, recents, autoSave])

  // The whole layer is the overlay, so a press on the dimmed window is not an
  // "outside" press (that would close on pointer-down and let the click
  // land on whatever is beneath); the backdrop closes on click instead.
  useOverlay({ open: active, ref: overlayRef, onDismiss: close })
  useFocusTrap({ open: active, ref: panelRef, initialFocus: inputRef, returnFocus: false })

  // `.onChange(of: scrollRequest)`: the selected row into view, scrolling as little as possible.
  useLayoutEffect(() => {
    if (scrollRequest === 0) return
    const row = results[selection]
    if (!row) return
    panelRef.current?.querySelector<HTMLElement>(`[data-row-id="${CSS.escape(row.id)}"]`)?.scrollIntoView({ block: 'nearest' })
    // Only a new request scrolls; `results` / `selection` changes alone do not.
  }, [scrollRequest])

  const updateQuery = (value: string) => {
    setQuery(value)
    setSelection(0)
    setScrollRequest((count) => count + 1)
  }

  const move = (delta: number) => {
    const count = results.length
    if (count === 0) return
    setSelection((current) => (((current + delta) % count) + count) % count)
    setScrollRequest((value) => value + 1)
  }

  const selected = (): PaletteRow | null => results[selection] ?? results[0] ?? null

  // The field's keys (`.onKeyPress(.downArrow / .upArrow / .escape)`,
  // `.onSubmit`). A native listener on the field itself: the focus trap stops
  // Return and Esc at the panel, before they would reach React's listener on
  // the portal's container, so a React `onKeyDown` would never see them.
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      move(1)
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      move(-1)
    } else if (event.key === 'Enter' && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault()
      event.stopPropagation()
      activate(selected())
    } else if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      close()
    }
  }
  const keyHandler = useRef(onKeyDown)
  useLayoutEffect(() => {
    keyHandler.current = onKeyDown
  })
  useEffect(() => {
    const input = inputRef.current
    if (!input) return
    const listener = (event: KeyboardEvent) => keyHandler.current(event)
    input.addEventListener('keydown', listener)
    return () => input.removeEventListener('keydown', listener)
  }, [])

  const hover = useCallback((index: number, event: MouseEvent<HTMLDivElement>) => {
    const location = { x: event.screenX, y: event.screenY }
    const last = lastHoverLocation.current
    if (last !== null && last.x === location.x && last.y === location.y) return
    lastHoverLocation.current = location
    setSelection(index)
  }, [])

  const trimmedForMessage = query.replace(/^[\s]+|[\s]+$/g, '')
  const groups = groupBySection(results)
  const activeRow = results[selection]

  return (
    <div ref={overlayRef} className={[styles.overlay, leaving ? styles.leaving : ''].filter(Boolean).join(' ')} role="presentation" aria-hidden={active ? undefined : true}>
      <div className={styles.backdrop} onClick={close} aria-hidden="true" />
      <div ref={panelRef} className={styles.panel} role="dialog" aria-label="Command palette" tabIndex={-1}>
        <div className={styles.searchRow}>
          <Icon name="search" size={14} strokeWidth={2} className={styles.searchIcon} />
          <input
            ref={inputRef}
            type="text"
            className={styles.searchInput}
            placeholder="Search chats and actions…"
            aria-label="Search chats and actions"
            role="combobox"
            aria-expanded={results.length > 0}
            aria-controls={listId}
            aria-activedescendant={activeRow ? optionId(listId, activeRow.id) : undefined}
            aria-autocomplete="list"
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            value={query}
            onChange={(event) => updateQuery(event.target.value)}
          />
          {query.length > 0 ? (
            <button
              type="button"
              className={styles.clearButton}
              aria-label="Clear search"
              onClick={() => {
                updateQuery('')
                inputRef.current?.focus()
              }}
            >
              <Icon name="clear" size={13} />
            </button>
          ) : null}
          <Kbd>{isMac ? 'esc' : 'Esc'}</Kbd>
        </div>
        <div className={styles.divider} aria-hidden="true" />
        {results.length === 0 ? (
          <div className={styles.noResults}>{`No chats or actions match “${trimmedForMessage}”`}</div>
        ) : (
          <div id={listId} className={styles.list} style={{ height: listHeight(results) }} role="listbox" aria-label="Chats and actions">
            <div className={styles.listContent}>
              {groups.map((group) => (
                <div key={group.section} className={styles.group} role="group" aria-labelledby={`${listId}-${group.section}`}>
                  <div id={`${listId}-${group.section}`} className={styles.sectionHeader} role="presentation">
                    {sectionTitle(group.section)}
                  </div>
                  {group.rows.map(({ row, index }) => (
                    <PaletteRowView
                      key={row.id}
                      id={optionId(listId, row.id)}
                      row={row}
                      index={index}
                      isSelected={index === selection}
                      onHover={hover}
                      onActivate={activate}
                    />
                  ))}
                </div>
              ))}
            </div>
          </div>
        )}
        <div className={styles.footer} aria-hidden="true">
          <Hint keys="↑↓" label="navigate" />
          <Hint keys={isMac ? '↩' : 'Enter'} label="open" />
          <Hint keys={isMac ? 'esc' : 'Esc'} label="close" />
        </div>
      </div>
    </div>
  )
}

function optionId(listId: string, rowId: string): string {
  return `${listId}-${rowId.replace(/[^A-Za-z0-9_-]/g, '_')}`
}

/** The rows of each section in order, with their index in the flat list. */
function groupBySection(results: PaletteRow[]): { section: PaletteSection; rows: { row: PaletteRow; index: number }[] }[] {
  const groups: { section: PaletteSection; rows: { row: PaletteRow; index: number }[] }[] = []
  results.forEach((row, index) => {
    const last = groups[groups.length - 1]
    if (last && last.section === row.section) last.rows.push({ row, index })
    else groups.push({ section: row.section, rows: [{ row, index }] })
  })
  return groups
}

function Hint({ keys, label }: { keys: string; label: string }) {
  return (
    <span className={styles.hint}>
      <Kbd>{keys}</Kbd>
      <span className={styles.hintLabel}>{label}</span>
    </span>
  )
}

/// `PaletteRowView`: a 24 px icon tile, the title (with the project chip when
/// several projects are open) over the snippet, and the relative time or the
/// action's shortcut at the right; `surfaceRaised` while selected.
const PaletteRowView = memo(function PaletteRowView({
  id,
  row,
  index,
  isSelected,
  onHover,
  onActivate,
}: {
  id: string
  row: PaletteRow
  index: number
  isSelected: boolean
  onHover: (index: number, event: MouseEvent<HTMLDivElement>) => void
  onActivate: (row: PaletteRow) => void
}) {
  return (
    <div
      id={id}
      data-row-id={row.id}
      role="option"
      aria-selected={isSelected}
      aria-label={row.snippet !== null ? `${row.title}. ${row.snippet}` : row.title}
      className={[styles.row, isSelected ? styles.rowSelected : ''].filter(Boolean).join(' ')}
      onMouseEnter={(event) => onHover(index, event)}
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => onActivate(row)}
    >
      <span className={[styles.rowIcon, isSelected ? styles.rowIconSelected : ''].filter(Boolean).join(' ')} aria-hidden="true">
        <Icon name={row.icon} size={13} strokeWidth={2} />
      </span>
      <span className={styles.rowText}>
        <span className={styles.rowTitleLine}>
          <span className={styles.rowTitle}>{row.title}</span>
          {row.subtitle !== null ? <Chip icon="folder">{row.subtitle}</Chip> : null}
        </span>
        {row.snippet !== null ? <span className={styles.rowSnippet}>{row.snippet}</span> : null}
      </span>
      <span className={styles.rowSpacer} />
      {row.trailingText ? <span className={styles.rowTrailing}>{row.trailingText}</span> : null}
      {row.trailing !== null ? (
        row.kind.kind === 'chat' ? (
          <span className={styles.rowTrailing}>{row.trailing}</span>
        ) : isMac && row.macTrailing ? (
          <Kbd>{row.macTrailing}</Kbd>
        ) : (
          <Kbd shortcut={row.trailing} />
        )
      ) : null}
    </div>
  )
}, sameRowProps)

/** Rows are rebuilt on every search; a row re-renders only when what it shows changes. */
function sameRowProps(
  a: { id: string; row: PaletteRow; index: number; isSelected: boolean; onHover: unknown; onActivate: unknown },
  b: { id: string; row: PaletteRow; index: number; isSelected: boolean; onHover: unknown; onActivate: unknown },
): boolean {
  return (
    a.id === b.id &&
    a.index === b.index &&
    a.isSelected === b.isSelected &&
    a.onHover === b.onHover &&
    a.onActivate === b.onActivate &&
    a.row.title === b.row.title &&
    a.row.snippet === b.row.snippet &&
    a.row.trailing === b.row.trailing &&
    a.row.macTrailing === b.row.macTrailing &&
    a.row.trailingText === b.row.trailingText &&
    a.row.subtitle === b.row.subtitle &&
    a.row.icon === b.row.icon &&
    sameKind(a.row.kind, b.row.kind)
  )
}

function sameKind(a: PaletteKind, b: PaletteKind): boolean {
  if (a.kind === 'chat' && b.kind === 'chat') return a.projectId === b.projectId && a.sessionId === b.sessionId
  if (a.kind === 'action' && b.kind === 'action') return a.action === b.action
  if (a.kind === 'recent' && b.kind === 'recent') return recentProjectId(a.recent) === recentProjectId(b.recent)
  return a.kind === 'clearRecents' && b.kind === 'clearRecents'
}

// MARK: - Activation

/** `close()`: hides the palette and gives the composer its focus back. */
function close(): void {
  const state = useApp.getState()
  state.setPaletteVisible(false)
  state.focusComposer()
}

function activate(row: PaletteRow | null): void {
  if (!row) return
  close()
  const state = useApp.getState()
  const kind = row.kind
  switch (kind.kind) {
    case 'chat':
      state.selectProject(kind.projectId, false)
      void state.selectSession(kind.sessionId)
      return
    case 'action':
      run(kind.action)
      return
    case 'recent':
      void state.openRecent(kind.recent)
      return
    case 'clearRecents':
      clearRecentProjects()
      return
  }
}

function run(action: PaletteAction): void {
  const state = useApp.getState()
  switch (action) {
    case 'newChat':
      state.newChat()
      return
    case 'newWorktree':
      newWorktreeChat()
      return
    case 'openProject':
      void state.chooseProject()
      return
    case 'openRemoteProject':
      state.chooseRemoteProject()
      return
    case 'switchModel':
      if (!state.modelPickerVisible) state.toggleModelPicker()
      return
    case 'rebuildIndex':
      void state.rebuildIndex()
      return
    case 'toggleInspector':
      state.toggleInspector()
      return
    case 'runDoctor':
      if (state.projects.some((project) => project.id === state.selectedProjectId)) state.setSidebarItem('project')
      void state.runDoctor(false)
      return
    case 'settings':
      state.openSettings()
      return
    case 'findInChat':
      findInChat()
      return
    case 'copyTranscript':
      copyTranscript()
      return
    case 'exportChat':
      void exportChatAsMarkdown()
      return
    case 'toggleAutoSave':
      setEditorAutoSave(!getPref('editorAutoSave'))
      return
    case 'toggleSidebar':
      state.toggleSidebar()
      return
    case 'goWelcome':
      state.setSidebarItem('welcome')
      return
    case 'goProjects':
      state.setSidebarItem('projects')
      return
    case 'goApps':
      state.setSidebarItem('apps')
      return
  }
}

// MARK: - Results

/** `query.trimmingCharacters(in: .whitespacesAndNewlines)`. */
function trimmed(text: string): string {
  return text.trim()
}

function paletteResults(query: string): PaletteRow[] {
  const needle = trimmed(query)
  return [...chatRows(needle), ...actionRows(needle)]
}

interface ScoredRow {
  row: PaletteRow
  score: number
  date: number
}

/// Chats: with no query, the selected project's eight most recent; with
/// one, up to twelve chats of every open project whose title or task text
/// matches (task matches rank slightly below title matches).
function chatRows(query: string): PaletteRow[] {
  const state = useApp.getState()
  const multipleProjects = state.projects.length > 1
  const scored: ScoredRow[] = []
  for (const project of state.projects) {
    const tasksBySession = new Map<string, TaskSummary[]>()
    for (const task of project.tasks) {
      const key = task.session_id ?? ''
      const list = tasksBySession.get(key)
      if (list) list.push(task)
      else tasksBySession.set(key, [task])
    }
    for (const session of project.sessions) {
      const tasks = tasksBySession.get(session.id) ?? []
      const subtitle = multipleProjects ? project.name : null
      const updated = sessionUpdatedDate(session)
      const base = {
        id: `chat-${project.id}-${session.id}`,
        section: 'chats' as const,
        title: session.title,
        trailing: TranscriptFormat.relative(updated),
        subtitle,
        icon: 'chat' as const,
        kind: { kind: 'chat' as const, projectId: project.id, sessionId: session.id },
      }
      if (query.length === 0) {
        if (project.id !== state.selectedProjectId) continue
        const first = tasks[0]
        const snippet = first ? (first.summary ?? first.objective) : null
        scored.push({ row: { ...base, snippet: snippet !== null ? ChatState.clip(snippet, 90) : null }, score: 0, date: updated.getTime() })
        continue
      }
      let best = ModelCatalog.fuzzyMatch(query, session.title)?.score ?? -1
      let snippet: string | null = null
      for (const task of tasks) {
        for (const text of [task.objective, task.summary ?? '']) {
          if (text.length === 0) continue
          const match = ModelCatalog.fuzzyMatch(query, text)
          if (!match) continue
          // Task text matches rank slightly below title matches.
          const score = match.score - 50
          if (score > best || (snippet === null && best < 0)) {
            best = Math.max(best, score)
            snippet = snippetAround(text, query)
          }
        }
      }
      if (best < 0) continue
      const first = tasks[0]
      if (snippet === null && first) snippet = ChatState.clip(first.summary ?? first.objective, 90)
      scored.push({ row: { ...base, snippet }, score: best, date: updated.getTime() })
    }
  }
  if (query.length === 0) {
    return scored
      .sort((a, b) => b.date - a.date)
      .slice(0, 8)
      .map((entry) => entry.row)
  }
  return scored
    .sort((a, b) => (a.score !== b.score ? b.score - a.score : b.date - a.date))
    .slice(0, 12)
    .map((entry) => entry.row)
}

/// Actions available now (some need a selected project or chat), then the
/// recent projects and Clear recent projects; fuzzy-matched on title and
/// detail.
function actionRows(query: string): PaletteRow[] {
  const state = useApp.getState()
  const hasProject = state.projects.some((project) => project.id === state.selectedProjectId)
  const hasChat = selectedChat(state) !== null
  const autoSave = getPref('editorAutoSave')
  const rows = PALETTE_ACTIONS.filter((action) => {
    const info = ACTION_INFO[action]
    return (!info.needsProject || hasProject) && (!info.needsChat || hasChat)
  }).map((action): PaletteRow => {
    const info = ACTION_INFO[action]
    return {
      id: `action-${action}`,
      section: 'actions',
      title: info.title,
      snippet: info.detail,
      trailing: info.shortcut,
      macTrailing: info.macShortcut,
      trailingText: action === 'toggleAutoSave' ? (autoSave ? 'On' : 'Off') : null,
      subtitle: null,
      icon: info.icon,
      kind: { kind: 'action', action },
    }
  })
  const recents = sortedRecentProjects(getPref('recents'))
  const recentRows = recents.map((recent): PaletteRow => {
    const where = recent.hostId === null ? abbreviatedPath(recent.path) : `${recent.path} on ${recent.hostName ?? 'a remote host'}`
    return {
      id: `recent-${recentProjectId(recent)}`,
      section: 'actions',
      title: `Open ${recent.name}`,
      snippet: recent.pinned ? `${where} · pinned` : where,
      trailing: null,
      subtitle: null,
      icon: recent.hostId === null ? 'folder' : 'remoteHost',
      kind: { kind: 'recent', recent },
    }
  })
  const clearRow: PaletteRow[] = recents.some((recent) => !recent.pinned)
    ? [{ id: 'action-clearRecents', section: 'actions', title: 'Clear recent projects', snippet: 'Forget every recent project that is not pinned', trailing: null, subtitle: null, icon: 'trash', kind: { kind: 'clearRecents' } }]
    : []
  if (query.length === 0) return [...rows, ...recentRows.slice(0, RECENTS_SHOWN), ...clearRow]
  const matched: { row: PaletteRow; score: number }[] = []
  for (const row of [...rows, ...recentRows, ...clearRow]) {
    const match = ModelCatalog.fuzzyMatch(query, `${row.title} ${row.snippet ?? ''}`)
    if (match) matched.push({ row, score: match.score })
  }
  return matched.sort((a, b) => b.score - a.score).map((entry) => entry.row)
}

/** Case- and diacritic-folded form of one character. */
function fold(char: string): string {
  return char.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase()
}

/// `CommandPalette.snippet(_:around:)`: a ~90 character window of `text`
/// around the first (case- and diacritic-insensitive) occurrence of `query`,
/// with 30 characters of lead and an ellipsis when it starts mid-text.
function snippetAround(text: string, query: string): string {
  const collapsed = ChatState.clip(text, 400)
  const chars = Array.from(collapsed)
  // The folded text, and for each of its UTF-16 offsets the character it came from.
  let folded = ''
  const owner: number[] = []
  chars.forEach((char, index) => {
    const piece = fold(char)
    folded += piece
    for (let i = 0; i < piece.length; i += 1) owner.push(index)
  })
  const foldedQuery = Array.from(query).map(fold).join('')
  const at = foldedQuery.length === 0 ? -1 : folded.indexOf(foldedQuery)
  if (at < 0) return ChatState.clip(collapsed, 90)
  const lower = owner[at] ?? 0
  const lead = 30
  const start = Math.max(0, lower - lead)
  let window = chars.slice(start).join('')
  if (start > 0) window = '…' + window
  return ChatState.clip(window, 90)
}
