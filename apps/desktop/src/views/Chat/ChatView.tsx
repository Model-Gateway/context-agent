// Port of apps/macos/ContextAgent/Views/Chat/ChatView.swift: the Chat tab:
// the committed transcript (a `TailWindow` with day separators), the live
// turn, auto-follow with a "Jump to latest" pill, the mod+F find bar, the
// model-mismatch notice, the Retry / Continue action row and the composer
// dock (the question card above the composer) at the bottom.

import { invoke } from '@tauri-apps/api/core'
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { executionStateLabel } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { IconButton } from '../../components/IconButton'
import { isModKey } from '../../components/platform'
import { Skeleton } from '../../components/Skeleton'
import { TailWindow, type TailWindowHandle } from '../../components/TailWindow'
import { Tooltip } from '../../components/Tooltip'
import { invalidateFileReferences, type FileReferenceContext } from '../../markdown/fileReferences'
import { useModelsDevVersion } from '../../models/modelsDev'
import { chatStatusProgress } from '../../models/reducer'
import { rowLabel } from '../../models/toolMetadata'
import {
  agentGroupRunningCount,
  assistantTurnIsEmpty,
  toolGroupRunningCount,
  transcriptItemDate,
  type TranscriptItem,
} from '../../models/transcript'
import { relativePath, useApp, useChatValue, useProjectValue } from '../../state/app'
import type { ChatState } from '../../state/chat'
import type { ProjectState } from '../../state/project'
import { onFindInChat } from '../../state/keys'
import { projectById, selectedChat, selectedProject, useNeedsAPIKey } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { ComposerView } from '../Composer/ComposerView'
import { AgentTimelineView } from './AgentTimelineView'
import { ApprovalCardView } from './ApprovalCardView'
import { DaySeparator } from './DaySeparator'
import { EmptyChatView } from './EmptyChatView'
import { FindBar } from './FindBar'
import { OutcomeCardView } from './OutcomeCardView'
import { QuestionCardView } from './QuestionCardView'
import { ToolTimelineView } from './ToolTimelineView'
import { AssistantMessageView, FileReferencesContext, NoticeView, TranscriptHighlight, TranscriptTime, UserMessageView } from './TranscriptItemViews'
import styles from './ChatView.module.css'

// MARK: - Chat tab

/**
 * `ChatTabView` (ChatView.swift): picks the Chat tab's content: the selected
 * chat, the empty state for the selected project, or a placeholder when no
 * project is open.
 */
export function ChatTabView() {
  const chat = useApp(selectedChat)
  const project = useApp(selectedProject)
  if (chat) return <ChatView key={chat.sessionId} chat={chat} />
  if (project) return <EmptyChatView key={project.id} project={project} />
  return (
    <div className={styles.placeholder}>
      <EmptyPlaceholder
        icon="folder"
        title="No project open"
        subtitle="Open a repository folder to start a chat."
        actionTitle="Open a project…"
        onAction={() => void useApp.getState().chooseProject()}
      />
    </div>
  )
}

// MARK: - Chat view

export interface ChatViewProps {
  chat: ChatState
}

/**
 * `ChatView` (ChatView.swift): one chat session: the find bar (when open),
 * the transcript pane, and the composer dock below, with the chat's
 * `FileReferenceContext` for every markdown view inside. The `RewindSheet`
 * is mounted by the window (Shell).
 */
export function ChatView({ chat }: ChatViewProps) {
  const project = useApp((state) => projectById(state, chat.projectId))
  const items = useTranscriptItems(chat)
  const files = useFileReferenceContext(project)

  const [findVisible, setFindVisible] = useState(false)
  const [findQuery, setFindQuery] = useState('')
  const [findCursor, setFindCursor] = useState(0)
  const trimmedQuery = findQuery.trim()
  const matches = useMemo(() => (findVisible ? findMatches(items, trimmedQuery) : []), [findVisible, items, trimmedQuery])
  const cursor = clampedCursor(findCursor, matches.length)
  const currentMatch = matches.length === 0 ? null : (matches[cursor] ?? null)

  const latest = useRef({ count: matches.length })
  useLayoutEffect(() => {
    latest.current = { count: matches.length }
  })

  const step = useCallback((delta: number) => {
    const count = latest.current.count
    if (count <= 0) return
    setFindCursor((current) => (((clampedCursor(current, count) + delta) % count) + count) % count)
  }, [])

  const showFind = useCallback(() => setFindVisible(true), [])

  const closeFind = useCallback(() => {
    setFindVisible(false)
    setFindCursor(0)
  }, [])

  const onQueryChange = useCallback((query: string) => {
    setFindQuery(query)
    // `.onChange(of: findQuery) { findCursor = 0 }`
    setFindCursor(0)
  }, [])

  // `.onReceive(findNotification)`: mod+F and the palette's "Find in Chat"
  // (state/keys.ts `findInChat`, which switches to the Chat tab first).
  useEffect(() => onFindInChat(showFind), [showFind])

  // mod+G / mod+shift+G cycle the matches while the bar is open (the view's
  // hidden shortcut buttons; mod+F itself is state/keys.ts').
  useEffect(() => {
    if (!findVisible) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || event.altKey || !isModKey(event)) return
      if (event.key.toLowerCase() !== 'g') return
      event.preventDefault()
      step(event.shiftKey ? -1 : 1)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [findVisible, step])

  return (
    <FileReferencesContext.Provider value={files}>
      <div className={styles.chat}>
        {findVisible ? (
          <FindBar
            query={findQuery}
            onQueryChange={onQueryChange}
            matchCount={matches.length}
            cursor={cursor}
            onPrevious={() => step(-1)}
            onNext={() => step(1)}
            onClose={closeFind}
          />
        ) : null}
        <TranscriptPane key={chat.sessionId} chat={chat} items={items} findQuery={findVisible ? trimmedQuery : ''} currentMatch={currentMatch} />
        <ComposerDock chat={chat} />
      </div>
    </FileReferencesContext.Provider>
  )
}

/// File names in the transcript resolve against the chat's project (and its
/// changed files) and open through `AppState.reveal`. Memoised on the
/// project root, like the Swift context's `==`, so a rebuild of the chat
/// does not redraw every message. Swift's `isFile` asks the disk
/// synchronously; here it answers from the Inspector's file tree and from
/// `FolderListings`, which lists a folder the first time a name in it is
/// asked about and then redraws the messages once (the context changes
/// identity only when a listing brought new files).
function useFileReferenceContext(project: ProjectState | null): FileReferenceContext | null {
  const root = project?.root ?? null
  const [listingVersion, setListingVersion] = useState(0)
  const listings = useMemo(() => (project ? new FolderListings(project) : null), [project])
  const gitStatus = useProjectValue(project, (p) => p.gitStatus, null)

  useEffect(() => {
    if (!listings) return
    listings.attach(() => setListingVersion((version) => version + 1))
    return () => listings.detach()
  }, [listings])

  // The working tree changed (`ProjectState` already forgot the resolved
  // references): re-read the folders listed so far.
  const firstStatus = useRef(true)
  useEffect(() => {
    if (firstStatus.current) {
      firstStatus.current = false
      return
    }
    void listings?.refresh()
  }, [gitStatus, listings])

  return useMemo(() => {
    if (!project || !listings || root === null) return null
    return {
      root,
      changedPaths: () => project.gitStatus?.files.map((file) => file.path) ?? [],
      isFile: (absolute: string) => listings.isFile(absolute),
      perform: (reference, action) => useApp.getState().reveal(reference.path, project, action, reference.line ?? null),
    }
    // `listingVersion`: a listing brought new files, so names resolve differently.
  }, [project, listings, root, listingVersion])
}

/** Folders listed for file references per chat, at most (stray paths in prose must not list the disk at will). */
const MAX_LISTED_FOLDERS = 100

/// The file names of the project folders that file references in the
/// transcript point into: the Inspector's tree where it has listed the
/// folder, else a listing of its own (`fs_list_dir`), read once per folder.
class FolderListings {
  /** Repository-relative folder ("" for the root) → its file names; null while being listed. */
  private readonly folders = new Map<string, ReadonlySet<string> | null>()
  private timer: number | null = null
  /** Redraws the chat's messages; null while no chat view shows them. */
  private onChange: (() => void) | null = null

  constructor(private readonly project: ProjectState) {}

  /** Whether `absolute` names a file in the project; false (for now) while its folder is being listed. */
  isFile(absolute: string): boolean {
    const relative = relativePath(absolute, this.project.root)
    if (relative === absolute || relative.length === 0) return false
    const segments = relative.split('/')
    if (segments.some((segment) => segment.length === 0 || segment === '.' || segment === '..')) return false
    const name = segments.pop() ?? ''
    const parent = segments.join('/')
    const tree = this.project.fileTree.children.get(parent)
    if (tree) return tree.some((node) => node.path === relative && !node.isDirectory)
    const listed = this.folders.get(parent)
    if (listed !== undefined) return listed?.has(name) ?? false
    if (this.folders.size < MAX_LISTED_FOLDERS) void this.list(parent)
    return false
  }

  /// Re-reads the folders listed so far (the working tree changed); redraws
  /// only when one of them changed.
  async refresh(): Promise<void> {
    let changed = false
    for (const [parent, names] of [...this.folders.entries()]) {
      if (names === null) continue
      const fresh = await this.read(parent)
      if (fresh.size !== names.size || [...fresh].some((name) => !names.has(name))) {
        this.folders.set(parent, fresh)
        changed = true
      }
    }
    if (changed) this.schedule()
  }

  attach(onChange: () => void): void {
    this.onChange = onChange
  }

  /** The chat view went away (or StrictMode re-runs its effects): listings may still arrive, nothing redraws. */
  detach(): void {
    this.onChange = null
    if (this.timer !== null) window.clearTimeout(this.timer)
    this.timer = null
  }

  private async list(parent: string): Promise<void> {
    this.folders.set(parent, null)
    const names = await this.read(parent)
    this.folders.set(parent, names)
    if (names.size > 0) this.schedule()
  }

  private async read(parent: string): Promise<ReadonlySet<string>> {
    try {
      const entries = await invoke<Array<{ name: string; is_dir: boolean }>>('fs_list_dir', { path: this.project.fileTree.absolutePath(parent) })
      return new Set(entries.filter((entry) => !entry.is_dir).map((entry) => entry.name))
    } catch {
      return new Set()
    }
  }

  /** One redraw for the listings that arrive together. */
  private schedule(): void {
    if (this.timer !== null || this.onChange === null) return
    this.timer = window.setTimeout(() => {
      this.timer = null
      invalidateFileReferences()
      this.onChange?.()
    }, 50)
  }
}

// MARK: - Transcript snapshot

/// The committed items as an array that changes identity only when the
/// transcript does (an item appended, replaced or removed), not on every
/// streamed delta: the rows and the find matches memoise on it, so a
/// running task redraws only the live turn (Swift rule 8).
interface TranscriptSnapshot {
  version: number
  items: readonly TranscriptItem[]
  revision: number
}

const snapshots = new WeakMap<ChatState, TranscriptSnapshot>()

function transcriptRevision(chat: ChatState): number {
  const current = chat.transcript.items
  const snapshot = snapshots.get(chat)
  if (!snapshot) {
    snapshots.set(chat, { version: chat.version, items: [...current], revision: 0 })
    return 0
  }
  if (snapshot.version === chat.version) return snapshot.revision
  snapshot.version = chat.version
  let same = snapshot.items.length === current.length
  for (let index = current.length - 1; same && index >= 0; index -= 1) {
    if (snapshot.items[index] !== current[index]) same = false
  }
  if (!same) {
    snapshot.items = [...current]
    snapshot.revision += 1
  }
  return snapshot.revision
}

function useTranscriptItems(chat: ChatState): readonly TranscriptItem[] {
  useApp(() => transcriptRevision(chat))
  return snapshots.get(chat)?.items ?? chat.transcript.items
}

// MARK: - Transcript pane

interface FindMatch {
  itemId: string
  occurrence: number
}

interface TranscriptPaneProps {
  chat: ChatState
  items: readonly TranscriptItem[]
  /** Find-in-chat query (empty when the find bar is closed). */
  findQuery: string
  currentMatch: FindMatch | null
}

/** Unloaded messages, while the user scrolls to them: exchanges of varied shapes, 132 px plus 28 px. */
const SKELETON_EXCHANGE_HEIGHT = 160
const SKELETON_SHAPES: ReadonlyArray<{ question: number; reply: number[] }> = [
  { question: 0.42, reply: [1, 0.94, 0.62] },
  { question: 0.3, reply: [0.97, 1, 0.88, 0.4] },
  { question: 0.55, reply: [0.9, 0.7] },
  { question: 0.36, reply: [1, 0.96, 0.8, 0.92, 0.5] },
]

function skeletonExchange(index: number): ReactNode {
  const shape = SKELETON_SHAPES[index % SKELETON_SHAPES.length] ?? SKELETON_SHAPES[0]
  if (!shape) return null
  return (
    <div className={styles.skeletonExchange}>
      <Skeleton.Exchange question={shape.question} reply={shape.reply} />
    </div>
  )
}

function itemKey(item: TranscriptItem): string {
  return item.id
}

/**
 * `TranscriptPane` (ChatView.swift): the scrolling transcript: header
 * (history skeleton, history error, model notice), the committed rows in a
 * `TailWindow` (pages of 20, at most 60, the rest as space at the average
 * row height with skeleton exchanges next to the loaded rows), the live
 * turn, the action row, and the "Jump to latest" pill over the bottom.
 */
function TranscriptPane({ chat, items, findQuery, currentMatch }: TranscriptPaneProps) {
  const handleRef = useRef<TailWindowHandle>(null)
  const [atBottom, setAtBottom] = useState(true)

  const running = useChatValue(chat, (c) => c.status.kind === 'running', false)
  const showActionRow = useChatValue(chat, actionRowVisible, false)

  const currentMatchId = currentMatch?.itemId ?? null
  const renderItem = useCallback(
    (item: TranscriptItem, index: number) => (
      <TranscriptEntry item={item} separator={separatorAt(items, index)} chat={chat} findQuery={findQuery} isCurrentMatch={item.id === currentMatchId} />
    ),
    [items, chat, findQuery, currentMatchId],
  )

  // A new run follows the bottom again (`onChange(of: chat.status.isRunning)`).
  const wasRunning = useRef(running)
  useEffect(() => {
    if (running && !wasRunning.current) handleRef.current?.jumpToLatest({ animated: true })
    wasRunning.current = running
  }, [running])

  // A find match: stop following and bring the item into view, loading a
  // page around it when it lies outside the loaded window.
  const itemsRef = useRef(items)
  useLayoutEffect(() => {
    itemsRef.current = items
  })
  const matchKey = currentMatch ? `${currentMatch.itemId}\u0000${currentMatch.occurrence}` : null
  useEffect(() => {
    if (matchKey === null || currentMatchId === null) return
    const handle = handleRef.current
    if (!handle) return
    const index = itemsRef.current.findIndex((item) => item.id === currentMatchId)
    if (index < 0) {
      handle.stopFollowing()
      return
    }
    handle.loadAround(index, { block: 'center', animated: true })
    // `matchKey` carries the occurrence: stepping within one item scrolls again.
  }, [matchKey, currentMatchId])

  const followIfNeeded = useCallback(() => handleRef.current?.followIfNeeded(), [])
  const jumpToLatest = useCallback(() => handleRef.current?.jumpToLatest({ animated: true }), [])

  return (
    <div className={styles.pane}>
      <TailWindow
        ref={handleRef}
        items={items}
        itemKey={itemKey}
        renderItem={renderItem}
        page={20}
        limit={60}
        rowGap={20}
        renderPlaceholder={skeletonExchange}
        placeholderRowHeight={SKELETON_EXCHANGE_HEIGHT}
        contentClassName={styles.content}
        onAtBottomChange={setAtBottom}
        header={<TranscriptHeader chat={chat} />}
        footer={
          <>
            <LiveTurnView chat={chat} onGrow={followIfNeeded} />
            {showActionRow ? <ActionRow chat={chat} /> : null}
            <div className={styles.bottomAnchor} aria-hidden="true" />
          </>
        }
      />
      <JumpToLatestPill visible={!atBottom} onClick={jumpToLatest} />
    </div>
  )
}

/** Retry / Continue / Open Settings after a failed, cancelled or blocked run. */
function actionRowVisible(chat: ChatState): boolean {
  const status = chat.status
  if (status.kind === 'running' || status.kind === 'loadingHistory') return false
  if (chat.lastError !== null) return true
  return status.kind === 'finished' && status.status !== 'completed'
}

/// A centred day label before the first item (unless today) and whenever
/// the calendar day changes (the item before a window's first row counts).
function separatorAt(items: readonly TranscriptItem[], index: number): string | null {
  const item = items[index]
  if (!item) return null
  const date = transcriptItemDate(item)
  const previous = index > 0 ? items[index - 1] : undefined
  if (previous) return TranscriptTime.sameDay(transcriptItemDate(previous), date) ? null : TranscriptTime.dayLabel(date)
  return TranscriptTime.isToday(date) ? null : TranscriptTime.dayLabel(date)
}

interface TranscriptEntryProps {
  item: TranscriptItem
  separator: string | null
  chat: ChatState
  findQuery: string
  isCurrentMatch: boolean
}

/// One committed item under its day separator (`TranscriptRows`' entry): a
/// row of its own so that what only the pane reads (follow state, run
/// status) does not rebuild every row.
const TranscriptEntry = memo(function TranscriptEntry({ item, separator, chat, findQuery, isCurrentMatch }: TranscriptEntryProps) {
  return (
    <div className={styles.entry}>
      {separator !== null ? <DaySeparator label={separator} /> : null}
      <TranscriptRow item={item} chat={chat} findQuery={findQuery} isCurrentMatch={isCurrentMatch} />
    </div>
  )
})

export interface TranscriptRowProps {
  item: TranscriptItem
  chat: ChatState
  /** Find-in-chat query (empty when the find bar is closed). */
  findQuery?: string
  /** True for the item the find cursor points at. */
  isCurrentMatch?: boolean
}

/** `TranscriptRow` (ChatView.swift): dispatches one committed `TranscriptItem` to its view; the current find match gets an accent ring. */
export const TranscriptRow = memo(function TranscriptRow({ item, chat, findQuery = '', isCurrentMatch = false }: TranscriptRowProps) {
  const highlight = findQuery.length === 0 ? null : findQuery
  let content: ReactNode
  switch (item.kind) {
    case 'user':
      content = <UserMessageView turn={item} highlight={highlight} chat={chat} />
      break
    case 'assistant':
      content = <AssistantMessageView turn={item} projectId={chat.projectId} />
      break
    case 'tools':
      content = <ToolTimelineView group={item} chat={chat} />
      break
    case 'agents':
      content = <AgentTimelineView group={item} chat={chat} />
      break
    case 'approval':
      content = <ApprovalCardView item={item} chat={chat} />
      break
    case 'outcome':
      content = <OutcomeCardView item={item} chat={chat} />
      break
    case 'notice':
      content = <NoticeView item={item} highlight={highlight} />
      break
  }
  return (
    <div className={styles.row}>
      {content}
      {isCurrentMatch ? <div className={styles.matchRing} aria-hidden="true" /> : null}
    </div>
  )
})

// MARK: - Header

/** The pane's header: the history skeleton, the history error and the model notice. */
function TranscriptHeader({ chat }: { chat: ChatState }) {
  const loading = useChatValue(chat, (c) => c.status.kind === 'loadingHistory', false)
  const historyError = useChatValue(chat, (c) => c.historyError, null)
  const sessionModel = useChatValue(chat, (c) => c.summary.model.trim(), '')
  const projectModel = useApp((state) => projectById(state, chat.projectId)?.info.model ?? '')
  const [dismissed, setDismissed] = useState(false)
  // "This chat was started with X; new messages use Y" when they differ (once per selection).
  const mismatch = sessionModel.length > 0 && projectModel.length > 0 && projectModel !== sessionModel
  return (
    <>
      {loading ? <Skeleton.Transcript /> : null}
      {historyError !== null ? (
        <div className={styles.historyError}>
          <Icon name="warning" size={12} strokeWidth={2} className={styles.historyErrorIcon} />
          <span className={`${styles.historyErrorText} selectable`}>Could not load history: {historyError}</span>
          <span className={styles.flexSpacer} />
          <Button size="small" onClick={() => void useApp.getState().selectSession(chat.sessionId)}>
            Retry
          </Button>
        </div>
      ) : null}
      {mismatch && !dismissed ? <ModelMismatchNotice sessionModel={sessionModel} projectModel={projectModel} onDismissed={() => setDismissed(true)} /> : null}
    </>
  )
}

/// `ModelMismatchNotice`: "This chat was started with <session model>; new
/// messages use <project model>", dismissible (it fades out).
function ModelMismatchNotice({ sessionModel, projectModel, onDismissed }: { sessionModel: string; projectModel: string; onDismissed: () => void }) {
  useModelsDevVersion()
  // `modelDisplayName` names models after the configured endpoint's catalogue.
  useApp((state) => state.config?.model.base_url ?? '')
  const [leaving, setLeaving] = useState(false)
  const app = useApp.getState()
  const dismiss = () => {
    if (leaving) return
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      onDismissed()
      return
    }
    setLeaving(true)
    window.setTimeout(onDismissed, 150)
  }
  return (
    <div className={[styles.mismatch, leaving ? styles.leaving : ''].filter(Boolean).join(' ')} role="group">
      <Icon name="info" size={11} strokeWidth={2} className={styles.mismatchIcon} />
      <span className={styles.mismatchText}>
        This chat was started with {app.modelDisplayName(sessionModel)}; new messages use {app.modelDisplayName(projectModel)}
      </span>
      <span className={styles.flexSpacer} />
      <IconButton icon="close" label="Dismiss" size={11} onClick={dismiss} />
    </div>
  )
}

// MARK: - Live turn

export interface LiveTurnViewProps {
  chat: ChatState
  /** Fires whenever the live content grows, so the pane can keep following. */
  onGrow?: () => void
}

/**
 * `LiveTurnView` (ChatView.swift): the streaming part of the current run:
 * the open tool group, the live sub-agents, then the streaming assistant
 * turn (or a compact "Investigating…" row while the model is being called).
 */
export function LiveTurnView({ chat, onGrow }: LiveTurnViewProps) {
  const live = useChatValue(chat, (c) => c.live, null)
  const liveTools = useChatValue(chat, (c) => c.liveTools, null)
  const liveAgents = useChatValue(chat, (c) => c.liveAgents, null)
  const label = useChatValue(chat, workingLabel, null)

  const growth = [
    live?.text.length ?? 0,
    live?.reasoning.length ?? 0,
    liveTools?.calls.length ?? 0,
    liveTools ? toolGroupRunningCount(liveTools) : 0,
    live?.isThinking ? 1 : 0,
    liveAgents?.agents.reduce((sum, agent) => sum + agent.calls.length, 0) ?? 0,
  ].join(':')
  const onGrowRef = useRef(onGrow)
  useLayoutEffect(() => {
    onGrowRef.current = onGrow
  })
  useEffect(() => {
    onGrowRef.current?.()
  }, [growth])

  return (
    <div className={styles.live}>
      {liveTools && liveTools.calls.length > 0 ? <ToolTimelineView group={liveTools} chat={chat} /> : null}
      {liveAgents && liveAgents.agents.length > 0 ? <AgentTimelineView group={liveAgents} chat={chat} /> : null}
      {live && !assistantTurnIsEmpty(live) ? (
        <AssistantMessageView turn={live} />
      ) : label !== null ? (
        <div className={styles.working}>
          <BrailleSpinner size={13} />
          <span className={styles.workingLabel}>{label}…</span>
        </div>
      ) : null}
    </div>
  )
}

/** "Preparing" / "Investigating" while nothing streams and no tool or agent runs. */
function workingLabel(chat: ChatState): string | null {
  const progress = chatStatusProgress(chat.status)
  if (!progress) return null
  if (chat.pendingApproval !== null || chat.pendingQuestion !== null) return null
  if (chat.liveTools && toolGroupRunningCount(chat.liveTools) > 0) return null
  if (chat.liveAgents && agentGroupRunningCount(chat.liveAgents) > 0) return null
  return executionStateLabel(progress.state)
}

// MARK: - Action row

/// `ActionRow`: Retry · Continue (blocked) · Open Settings → Model (when the
/// endpoint is the likely cause: no key at all, an auth failure, or a probe
/// that could not reach or use the endpoint).
function ActionRow({ chat }: { chat: ChatState }) {
  const hasLastUserText = useChatValue(chat, (c) => c.transcript.lastUserText !== null, false)
  const blocked = useChatValue(chat, (c) => c.status.kind === 'finished' && c.status.status === 'blocked', false)
  const errorKind = useChatValue(chat, (c) => c.lastErrorKind, null)
  const needsKey = useNeedsAPIKey()
  const showsModelSettings = needsKey || errorKind === 'provider_auth' || errorKind === 'provider_unsupported' || errorKind === 'provider_connection'
  return (
    <div className={styles.actionRow}>
      <Tooltip content="Send the last message again">
        <Button size="small" icon="refresh" disabled={!hasLastUserText} onClick={() => void useApp.getState().retry(chat)}>
          Retry
        </Button>
      </Tooltip>
      {blocked ? (
        <Tooltip content="Ask the agent to continue with the unresolved items">
          <Button size="small" onClick={() => void useApp.getState().send('Continue with the unresolved items')}>
            Continue
          </Button>
        </Tooltip>
      ) : null}
      {showsModelSettings ? (
        <Button size="small" onClick={() => useApp.getState().openSettings('model')}>
          Open Settings → Model
        </Button>
      ) : null}
    </div>
  )
}

// MARK: - Jump to latest

/** `JumpToLatestPill`: a floating capsule over the transcript's bottom while the view is away from it. */
function JumpToLatestPill({ visible, onClick }: { visible: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      className={[styles.pill, visible ? styles.pillShown : ''].filter(Boolean).join(' ')}
      onClick={onClick}
      aria-label="Jump to latest"
      aria-hidden={!visible}
      tabIndex={visible ? 0 : -1}
    >
      <Icon name="arrowDown" size={11} strokeWidth={2.5} />
      <span>Jump to latest</span>
    </button>
  )
}

// MARK: - Composer dock

/// `ComposerDock`: the composer, centred under the transcript, with the
/// agent's question card above it while one waits. The "Queued · N" and
/// "Steering · N" rows are part of `ComposerView({ chat })`, as in Swift.
function ComposerDock({ chat }: { chat: ChatState }) {
  // Always mounted: the card renders nothing without a pending question and
  // runs its own enter / exit transition (`.opacity` + `.move(edge: .bottom)`),
  // the 10 pt VStack spacing included.
  return (
    <div className={styles.dock}>
      <div className={styles.dockColumn}>
        <QuestionCardView chat={chat} />
        <ComposerView chat={chat} />
      </div>
    </div>
  )
}

// MARK: - Find

function clampedCursor(cursor: number, count: number): number {
  if (count <= 0) return 0
  return ((cursor % count) + count) % count
}

/** `FindMatch.all(in:query:)`: one match per occurrence of `query` in each item's searchable text. */
function findMatches(items: readonly TranscriptItem[], query: string): FindMatch[] {
  if (query.length === 0) return []
  const out: FindMatch[] = []
  for (const item of items) {
    const count = TranscriptHighlight.occurrences(query, searchableText(item))
    for (let occurrence = 0; occurrence < count; occurrence += 1) out.push({ itemId: item.id, occurrence })
  }
  return out
}

/** `FindMatch.searchableText(_:)`. */
function searchableText(item: TranscriptItem): string {
  switch (item.kind) {
    case 'user':
    case 'assistant':
      return item.text
    case 'tools':
      return item.calls.map((call) => rowLabel(call) + ' ' + call.summary).join('\n')
    case 'agents':
      return item.agents
        .map((agent) =>
          [agent.name, agent.objective, agent.report?.summary ?? '', ...agent.calls.map((call) => rowLabel(call) + ' ' + call.summary)].join('\n'),
        )
        .join('\n')
    case 'approval':
      return item.request.summary + '\n' + item.request.details
    case 'outcome':
      return item.outcome.summary + '\n' + item.outcome.modified_files.join('\n')
    case 'notice':
      return [...item.lines, ...item.detailLines].join('\n')
  }
}
